use crate::handle_table::HandleTable;
use crate::http_body;
use wasi::http::types::{IncomingResponse, OutgoingResponse, ResponseOutparam};

use core::cell::RefCell;

thread_local! {
    static INCOMING_RESP_TABLE: RefCell<HandleTable<IncomingResponse>> = const { RefCell::new(HandleTable::new()) };
    static OUTGOING_RESP_TABLE: RefCell<HandleTable<OutgoingResponse>> = const { RefCell::new(HandleTable::new()) };
    static RESPONSE_OUTPARAM: RefCell<Option<ResponseOutparam>> = const { RefCell::new(None) };
}

fn with_incoming_resp<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<IncomingResponse>) -> R,
{
    INCOMING_RESP_TABLE.with(|t| f(&mut t.borrow_mut()))
}

fn with_outgoing_resp<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<OutgoingResponse>) -> R,
{
    OUTGOING_RESP_TABLE.with(|t| f(&mut t.borrow_mut()))
}

// === Incoming Response ===

/// Register an incoming response in the table.
pub(crate) fn insert_incoming_response(resp: IncomingResponse) -> i32 {
    with_incoming_resp(|t| t.insert(resp))
}

/// Get the status code of an incoming response.
#[no_mangle]
pub extern "C" fn host_api_incoming_response_status(handle: i32) -> u16 {
    with_incoming_resp(|t| {
        let resp = t.get(handle).expect("invalid incoming response handle");
        resp.status()
    })
}

/// Get the headers handle from an incoming response.
#[no_mangle]
pub extern "C" fn host_api_incoming_response_headers(handle: i32) -> i32 {
    with_incoming_resp(|t| {
        let resp = t.get(handle).expect("invalid incoming response handle");
        let fields = resp.headers();
        crate::http_headers::insert_headers(fields)
    })
}

/// Get the body from an incoming response.
/// Returns an incoming body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_incoming_response_body(handle: i32) -> i32 {
    with_incoming_resp(|t| {
        let resp = t.get(handle).expect("invalid incoming response handle");
        match resp.consume() {
            Ok(body) => http_body::insert_incoming_body(body),
            Err(_) => -1,
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
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_make(status: u16, headers_handle: i32) -> i32 {
    let headers =
        crate::http_headers::remove_headers(headers_handle).expect("invalid headers handle");

    let resp = OutgoingResponse::new(headers);
    if status != 200 {
        resp.set_status_code(status)
            .expect("failed to set status code");
    }
    with_outgoing_resp(|t| t.insert(resp))
}

/// Get the headers handle from an outgoing response.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_headers(handle: i32) -> i32 {
    with_outgoing_resp(|t| {
        let resp = t.get(handle).expect("invalid outgoing response handle");
        let fields = resp.headers();
        crate::http_headers::insert_headers(fields)
    })
}

/// Get the body from an outgoing response.
/// Returns an outgoing body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_body(handle: i32) -> i32 {
    with_outgoing_resp(|t| {
        let resp = t.get(handle).expect("invalid outgoing response handle");
        match resp.body() {
            Ok(body) => http_body::insert_outgoing_body(body),
            Err(_) => -1,
        }
    })
}

/// Send an outgoing response via the response outparam.
/// Takes ownership of the outgoing response handle.
/// Returns true on success.
#[no_mangle]
pub extern "C" fn host_api_outgoing_response_send(handle: i32) -> bool {
    let resp = with_outgoing_resp(|t| t.remove(handle).expect("invalid outgoing response handle"));

    RESPONSE_OUTPARAM.with(|cell| {
        if let Some(outparam) = cell.borrow_mut().take() {
            ResponseOutparam::set(outparam, Ok(resp));
            true
        } else {
            false
        }
    })
}

/// Store the response outparam for the current request.
/// Called from the exports module when an incoming request arrives.
pub(crate) fn set_response_outparam(outparam: ResponseOutparam) {
    RESPONSE_OUTPARAM.with(|cell| {
        *cell.borrow_mut() = Some(outparam);
    });
}
