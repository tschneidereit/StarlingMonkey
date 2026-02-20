use crate::handle_table::HandleTable;
use crate::http_body;
use wasip3::http::types::{ErrorCode, Fields, Response};
use wit_bindgen::rt::async_support::StreamReader;

use core::cell::RefCell;

/// State for an incoming (received) response.
enum IncomingResponseState {
    Available(Response),
    Consumed,
}

/// State for an outgoing response (what we will return from the handler).
struct OutgoingResponseParts {
    status: u16,
    headers_handle: i32,
    /// The read side of the body stream. Created when body() is called.
    body_reader: Option<StreamReader<u8>>,
}

thread_local! {
    static INCOMING_RESP_TABLE: RefCell<HandleTable<IncomingResponseState>> = RefCell::new(HandleTable::new());
    static OUTGOING_RESP_TABLE: RefCell<HandleTable<OutgoingResponseParts>> = RefCell::new(HandleTable::new());
    /// The pending Response to be returned from the async handler.
    static PENDING_RESPONSE: RefCell<Option<Result<Response, ErrorCode>>> = RefCell::new(None);
}

fn with_incoming_resp<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<IncomingResponseState>) -> R,
{
    INCOMING_RESP_TABLE.with(|t| f(&mut t.borrow_mut()))
}

fn with_outgoing_resp<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<OutgoingResponseParts>) -> R,
{
    OUTGOING_RESP_TABLE.with(|t| f(&mut t.borrow_mut()))
}

// === Incoming Response ===

/// Register an incoming response in the table.
pub(crate) fn insert_incoming_response(resp: Response) -> i32 {
    with_incoming_resp(|t| t.insert(IncomingResponseState::Available(resp)))
}

/// Get the status code of an incoming response.
#[no_mangle]
pub extern "C" fn host_api_incoming_response_status(handle: i32) -> u16 {
    with_incoming_resp(|t| {
        let state = t.get(handle).expect("invalid incoming response handle");
        match state {
            IncomingResponseState::Available(r) => r.get_status_code(),
            IncomingResponseState::Consumed => panic!("response already consumed"),
        }
    })
}

/// Get the headers handle from an incoming response.
#[no_mangle]
pub extern "C" fn host_api_incoming_response_headers(handle: i32) -> i32 {
    with_incoming_resp(|t| {
        let state = t.get(handle).expect("invalid incoming response handle");
        let resp = match state {
            IncomingResponseState::Available(r) => r,
            IncomingResponseState::Consumed => panic!("response already consumed"),
        };
        let fields = resp.get_headers();
        crate::http_headers::insert_headers(fields)
    })
}

/// Consume the body from an incoming response.
/// Returns an incoming body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_incoming_response_body(handle: i32) -> i32 {
    with_incoming_resp(|t| {
        let state = t.get_mut(handle).expect("invalid incoming response handle");
        match core::mem::replace(state, IncomingResponseState::Consumed) {
            IncomingResponseState::Available(resp) => {
                // Create an error future for consume_body.
                // wit_future::new returns (FutureWriter, FutureReader).
                let (error_writer, error_reader) =
                    wasip3::wit_future::new::<Result<(), ErrorCode>>(|| Ok(()));

                let (body_reader, _trailers_reader) =
                    Response::consume_body(resp, error_reader);

                // We can drop the error writer (no errors to signal).
                drop(error_writer);

                http_body::insert_incoming_body(body_reader)
            }
            IncomingResponseState::Consumed => -1,
        }
    })
}

/// Drop an incoming response handle.
#[no_mangle]
pub extern "C" fn host_api_incoming_response_drop(handle: i32) {
    with_incoming_resp(|t| {
        t.remove(handle);
    });
}

// === Outgoing Response ===

/// Create an outgoing response with the given status code and headers.
/// Takes ownership of the headers handle.
/// Returns an outgoing response handle, or -1 on error.
///
/// Matches the wasip2 FFI signature: `(status: u16, headers_handle: i32) -> i32`.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_make(status: u16, headers_handle: i32) -> i32 {
    with_outgoing_resp(|t| {
        t.insert(OutgoingResponseParts {
            status,
            headers_handle,
            body_reader: None,
        })
    })
}

/// Get the headers handle from an outgoing response.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_headers(handle: i32) -> i32 {
    with_outgoing_resp(|t| {
        let parts = t.get(handle).expect("invalid outgoing response handle");
        // Re-get the headers from the stored handle.
        // The headers are still in the headers table since we only stored the handle.
        crate::http_headers::clone_headers(parts.headers_handle)
    })
}

/// Get the body from an outgoing response (creates a stream for writing).
/// Returns an outgoing body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_body(handle: i32) -> i32 {
    // Create a body stream pair.
    // wit_stream::new returns (StreamWriter, StreamReader).
    let (body_writer, body_reader) = wasip3::wit_stream::new::<u8>();

    // Store the reader on the outgoing response parts.
    with_outgoing_resp(|t| {
        let parts = t.get_mut(handle).expect("invalid outgoing response handle");
        parts.body_reader = Some(body_reader);
    });

    // Return a body writer handle.
    http_body::insert_outgoing_body(body_writer)
}

/// Send an outgoing response.
/// In WASIp3, this builds the `Response` object from parts and stores it
/// as the pending response for the async handler to return.
/// Takes ownership of the outgoing response handle.
/// Returns true on success.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_send(handle: i32) -> bool {
    let parts = with_outgoing_resp(|t| {
        t.remove(handle).expect("invalid outgoing response handle")
    });

    // Get the headers (take ownership).
    let headers = match crate::http_headers::remove_headers(parts.headers_handle) {
        Some(h) => h,
        None => return false,
    };

    // Create trailers future (no trailers).
    // wit_future::new returns (FutureWriter, FutureReader).
    // Drop the writer; reader returns default Ok(None).
    let (trailers_writer, trailers_reader) =
        wasip3::wit_future::new::<Result<Option<Fields>, ErrorCode>>(|| Ok(None));
    drop(trailers_writer);

    // Build the Response.
    let (response, _send_result) =
        Response::new(headers, parts.body_reader, trailers_reader);

    // Set the status code.
    let _ = response.set_status_code(parts.status);

    // Store as pending for the handler to return.
    PENDING_RESPONSE.with(|cell| {
        *cell.borrow_mut() = Some(Ok(response));
    });

    true
}

/// Take the pending response (called by exports handler after starling_handle_request).
pub(crate) fn take_pending_response() -> Option<Result<Response, ErrorCode>> {
    PENDING_RESPONSE.with(|cell| cell.borrow_mut().take())
}

/// Drop an outgoing response handle without sending.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_drop(handle: i32) {
    with_outgoing_resp(|t| {
        t.remove(handle);
    });
}
