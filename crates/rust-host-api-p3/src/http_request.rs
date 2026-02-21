use crate::handle_table::HandleTable;
use crate::http_body;
use std::future::Future;
use std::pin::Pin;
use wasip3::http::types::{ErrorCode, Fields, Method, Request, Response, Scheme};
use wit_bindgen::rt::async_support::{FutureReader, FutureWriter, StreamWriter};

use core::cell::RefCell;

/// State for an incoming request — wraps the wasip3 Request before body is consumed.
enum IncomingRequestState {
    /// Request is available, body not yet consumed.
    Available(Request),
    /// Headers were taken, body was consumed. We keep headers ref around.
    Consumed,
}

thread_local! {
    static INCOMING_REQ_TABLE: RefCell<HandleTable<IncomingRequestState>> = const { RefCell::new(HandleTable::new()) };
    static OUTGOING_REQ_TABLE: RefCell<HandleTable<OutgoingRequestParts>> = const { RefCell::new(HandleTable::new()) };
    static FUTURE_RESP_TABLE: RefCell<HandleTable<FutureResponseState>> = const { RefCell::new(HandleTable::new()) };
    /// Save the request's FutureReader for error signaling (from consume_body).
    static REQUEST_ERROR_WRITERS: RefCell<HandleTable<FutureWriter<Result<(), ErrorCode>>>> = const { RefCell::new(HandleTable::new()) };
}

/// Parts of an outgoing request before it's sent.
struct OutgoingRequestParts {
    request: Request,
    /// The writer for the body stream (we write body data here).
    body_writer: Option<StreamWriter<u8>>,
    /// The FutureReader for the transmission result.
    _send_result: Option<FutureReader<Result<(), ErrorCode>>>,
}

struct FutureResponseState {
    /// The unawaited send future, stored here and later taken by the event loop.
    send_future: Option<PendingResponse>,
    /// Populated after client::send completes (set by the event loop).
    response: Option<Response>,
    /// Set to true if the send failed.
    error: bool,
}

fn with_incoming_req<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<IncomingRequestState>) -> R,
{
    INCOMING_REQ_TABLE.with(|t| f(&mut t.borrow_mut()))
}

fn with_outgoing_req<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<OutgoingRequestParts>) -> R,
{
    OUTGOING_REQ_TABLE.with(|t| f(&mut t.borrow_mut()))
}

fn with_future_resp<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<FutureResponseState>) -> R,
{
    FUTURE_RESP_TABLE.with(|t| f(&mut t.borrow_mut()))
}

/// FFI struct for a string result.
#[repr(C)]
pub struct HostApiString {
    pub ptr: *mut u8,
    pub len: usize,
}

// === Incoming Request ===

/// Register an incoming request in the table.
pub(crate) fn insert_incoming_request(req: Request) -> i32 {
    with_incoming_req(|t| t.insert(IncomingRequestState::Available(req)))
}

/// Get the HTTP method of an incoming request.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_method(handle: i32, out: *mut HostApiString) {
    with_incoming_req(|t| {
        let state = t.get(handle).expect("invalid incoming request handle");
        let req = match state {
            IncomingRequestState::Available(r) => r,
            IncomingRequestState::Consumed => panic!("request already consumed"),
        };
        let method = req.get_method();
        let method_str = match method {
            Method::Get => "GET".to_string(),
            Method::Head => "HEAD".to_string(),
            Method::Post => "POST".to_string(),
            Method::Put => "PUT".to_string(),
            Method::Delete => "DELETE".to_string(),
            Method::Connect => "CONNECT".to_string(),
            Method::Options => "OPTIONS".to_string(),
            Method::Trace => "TRACE".to_string(),
            Method::Patch => "PATCH".to_string(),
            Method::Other(s) => s,
        };
        let mut bytes = method_str.into_bytes().into_boxed_slice();
        let result = HostApiString {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
        };
        core::mem::forget(bytes);
        unsafe { *out = result };
    });
}

/// Get the headers handle from an incoming request.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_headers(handle: i32) -> i32 {
    with_incoming_req(|t| {
        let state = t.get(handle).expect("invalid incoming request handle");
        let req = match state {
            IncomingRequestState::Available(r) => r,
            IncomingRequestState::Consumed => panic!("request already consumed"),
        };
        let fields = req.get_headers();
        crate::http_headers::insert_headers(fields)
    })
}

/// Get the body from an incoming request.
/// In WASIp3, this consumes the request (moves it) via `consume_body`.
/// Returns an incoming body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_body(handle: i32) -> i32 {
    with_incoming_req(|t| {
        let state = t.get_mut(handle).expect("invalid incoming request handle");
        match core::mem::replace(state, IncomingRequestState::Consumed) {
            IncomingRequestState::Available(req) => {
                // Create an error future for consume_body.
                // wit_future::new returns (FutureWriter, FutureReader).
                let (error_writer, error_reader) =
                    wasip3::wit_future::new::<Result<(), ErrorCode>>(|| Ok(()));

                let (body_reader, _trailers_reader) = Request::consume_body(req, error_reader);

                // Store the error writer so we can signal errors later
                REQUEST_ERROR_WRITERS.with(|w| {
                    w.borrow_mut().insert(error_writer);
                });

                http_body::insert_incoming_body(body_reader)
            }
            IncomingRequestState::Consumed => -1,
        }
    })
}

/// Get the scheme of an incoming request.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_scheme(handle: i32, out: *mut HostApiString) {
    with_incoming_req(|t| {
        let state = t.get(handle).expect("invalid incoming request handle");
        let req = match state {
            IncomingRequestState::Available(r) => r,
            IncomingRequestState::Consumed => panic!("request already consumed"),
        };
        let scheme_str = match req.get_scheme() {
            Some(Scheme::Http) => "http".to_string(),
            Some(Scheme::Https) => "https".to_string(),
            Some(Scheme::Other(s)) => s,
            // wasmtime serve doesn't always set the scheme on p3 requests;
            // default to "http".
            None => "http".to_string(),
        };
        let mut bytes = scheme_str.into_bytes().into_boxed_slice();
        let result = HostApiString {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
        };
        core::mem::forget(bytes);
        unsafe { *out = result };
    });
}

/// Get the authority of an incoming request.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_authority(handle: i32, out: *mut HostApiString) {
    with_incoming_req(|t| {
        let state = t.get(handle).expect("invalid incoming request handle");
        let req = match state {
            IncomingRequestState::Available(r) => r,
            IncomingRequestState::Consumed => panic!("request already consumed"),
        };
        let auth = req.get_authority().unwrap_or_else(|| {
            // wasmtime serve doesn't always set the authority on p3 requests;
            // fall back to the Host header.
            let headers = req.get_headers();
            let host_values = headers.get("host");
            if let Some(first) = host_values.first() {
                String::from_utf8_lossy(first).into_owned()
            } else {
                "localhost".to_string()
            }
        });
        let mut bytes = auth.into_bytes().into_boxed_slice();
        let result = HostApiString {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
        };
        core::mem::forget(bytes);
        unsafe { *out = result };
    });
}

/// Get the path-with-query of an incoming request.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_path_with_query(handle: i32, out: *mut HostApiString) {
    with_incoming_req(|t| {
        let state = t.get(handle).expect("invalid incoming request handle");
        let req = match state {
            IncomingRequestState::Available(r) => r,
            IncomingRequestState::Consumed => panic!("request already consumed"),
        };
        let path = req.get_path_with_query().unwrap_or_else(|| "/".to_string());
        let mut bytes = path.into_bytes().into_boxed_slice();
        let result = HostApiString {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
        };
        core::mem::forget(bytes);
        unsafe { *out = result };
    });
}

/// Drop an incoming request handle.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_drop(handle: i32) {
    with_incoming_req(|t| {
        t.remove(handle);
    });
}

// === Outgoing Request ===

/// Create an outgoing request.
#[no_mangle]
pub unsafe extern "C" fn host_api_outgoing_request_make(
    method_ptr: *const u8,
    method_len: usize,
    headers_handle: i32,
    has_url: bool,
    scheme_ptr: *const u8,
    scheme_len: usize,
    authority_ptr: *const u8,
    authority_len: usize,
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    let method_str =
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(method_ptr, method_len));

    // Take ownership of the Fields from the headers table.
    let headers =
        crate::http_headers::remove_headers(headers_handle).expect("invalid headers handle");

    // Create a body stream for the request.
    // wit_stream::new returns (StreamWriter, StreamReader).
    let (body_writer, body_reader) = wasip3::wit_stream::new::<u8>();

    // Create trailers future (no trailers).
    // wit_future::new returns (FutureWriter, FutureReader).
    // Drop the writer immediately; the reader will return the default Ok(None).
    let (trailers_writer, trailers_reader) =
        wasip3::wit_future::new::<Result<Option<Fields>, ErrorCode>>(|| Ok(None));
    drop(trailers_writer);

    // Create the request.
    let (req, send_result) = Request::new(headers, Some(body_reader), trailers_reader, None);

    // Set method.
    let method = match method_str {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "CONNECT" => Method::Connect,
        "OPTIONS" => Method::Options,
        "TRACE" => Method::Trace,
        "PATCH" => Method::Patch,
        _ => Method::Other(method_str.to_string()),
    };
    let _ = req.set_method(&method);

    if has_url {
        let scheme_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(scheme_ptr, scheme_len));
        let scheme = match scheme_str {
            "http" | "http:" => Scheme::Http,
            "https" | "https:" => Scheme::Https,
            s => Scheme::Other(s.to_string()),
        };
        let _ = req.set_scheme(Some(&scheme));

        let authority = core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            authority_ptr,
            authority_len,
        ));
        let _ = req.set_authority(Some(authority));

        let path = core::str::from_utf8_unchecked(core::slice::from_raw_parts(path_ptr, path_len));
        let _ = req.set_path_with_query(Some(path));
    }

    with_outgoing_req(|t| {
        t.insert(OutgoingRequestParts {
            request: req,
            body_writer: Some(body_writer),
            _send_result: Some(send_result),
        })
    })
}

/// Get a headers handle from an outgoing request.
#[no_mangle]
pub extern "C" fn host_api_outgoing_request_headers(handle: i32) -> i32 {
    with_outgoing_req(|t| {
        let parts = t.get(handle).expect("invalid outgoing request handle");
        let fields = parts.request.get_headers();
        crate::http_headers::insert_headers(fields)
    })
}

/// Get the body from an outgoing request.
/// Returns an outgoing body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_outgoing_request_body(handle: i32) -> i32 {
    with_outgoing_req(|t| {
        let parts = t.get_mut(handle).expect("invalid outgoing request handle");
        if let Some(writer) = parts.body_writer.take() {
            http_body::insert_outgoing_body(writer)
        } else {
            -1 // body already taken
        }
    })
}

/// Send an outgoing request. Takes ownership of the request handle.
/// Returns a future incoming response handle, or -1 on error.
///
/// The send future is stored unawaited in the FutureResponseState.
/// The event loop will include it in its `select_all` race and update
/// the state when it completes.
#[no_mangle]
pub extern "C" fn host_api_outgoing_request_send(handle: i32) -> i32 {
    let parts = with_outgoing_req(|t| t.remove(handle).expect("invalid outgoing request handle"));

    // Drop body writer if still held (closes the empty stream for GET requests).
    drop(parts.body_writer);

    let send_fut = Box::pin(wasip3::http::client::send(parts.request));

    with_future_resp(|t| {
        t.insert(FutureResponseState {
            send_future: Some(send_fut),
            response: None,
            error: false,
        })
    })
}

// === Future Incoming Response ===

/// Subscribe to a future incoming response for readiness.
/// Returns a waiter handle that the event loop checks.
#[no_mangle]
pub extern "C" fn host_api_future_response_subscribe(handle: i32) -> i32 {
    crate::poll::register_future_response_waiter(handle)
}

/// Unsubscribe from a future response waiter.
#[no_mangle]
pub extern "C" fn host_api_future_response_unsubscribe(_handle: i32) {
    // Waiter will be cleaned up when future is dropped.
}

/// Try to get the response from a future.
/// Returns: handle >= 0 means response ready, -1 means not ready, -2 means error.
#[no_mangle]
pub extern "C" fn host_api_future_response_get(handle: i32) -> i32 {
    with_future_resp(|t| {
        let state = t.get_mut(handle).expect("invalid future response handle");
        if state.error {
            -2 // error
        } else if let Some(resp) = state.response.take() {
            crate::http_response::insert_incoming_response(resp)
        } else {
            -1 // not ready or already consumed
        }
    })
}

/// Drop a future incoming response handle.
#[no_mangle]
pub extern "C" fn host_api_future_response_drop(handle: i32) {
    with_future_resp(|t| {
        t.remove(handle);
    });
}

// === Async helpers for the event loop ===

/// Check if a future response is ready (response arrived or error set).
pub(crate) fn future_response_is_ready(handle: i32) -> bool {
    with_future_resp(|t| {
        t.get(handle)
            .is_none_or(|s| s.response.is_some() || s.error)
    })
}

pub type PendingResponse = Pin<Box<dyn Future<Output = Result<Response, ErrorCode>>>>;

/// Take the stored send future out of the state so the event loop can await it.
/// Returns `None` if the send has already been taken or completed.
pub(crate) fn take_send_future(
    handle: i32,
) -> Option<PendingResponse> {
    with_future_resp(|t| t.get_mut(handle).and_then(|s| s.send_future.take()))
}

/// Store the completed send result back into the FutureResponseState.
pub(crate) fn complete_send(handle: i32, result: Result<Response, ErrorCode>) {
    with_future_resp(|t| {
        if let Some(state) = t.get_mut(handle) {
            match result {
                Ok(response) => state.response = Some(response),
                Err(_) => state.error = true,
            }
        }
    })
}

/// Restore a send future back into state after cancellation (e.g. when another
/// future wins in `select_all` and this one gets dropped before completing).
pub(crate) fn restore_send_future(
    handle: i32,
    fut: PendingResponse,
) {
    with_future_resp(|t| {
        if let Some(state) = t.get_mut(handle) {
            state.send_future = Some(fut);
        }
    })
}
