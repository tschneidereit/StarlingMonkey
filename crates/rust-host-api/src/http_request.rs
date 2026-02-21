use crate::handle_table::HandleTable;
use crate::http_body;
use wasi::http::outgoing_handler;
use wasi::http::types::{FutureIncomingResponse, IncomingRequest, Method, OutgoingRequest, Scheme};

use core::cell::RefCell;

thread_local! {
    static INCOMING_REQ_TABLE: RefCell<HandleTable<IncomingRequest>> = const { RefCell::new(HandleTable::new()) };
    static OUTGOING_REQ_TABLE: RefCell<HandleTable<OutgoingRequest>> = const { RefCell::new(HandleTable::new()) };
    static FUTURE_RESP_TABLE: RefCell<HandleTable<FutureResponseState>> = const { RefCell::new(HandleTable::new()) };
}

struct FutureResponseState {
    future: FutureIncomingResponse,
    pollable_handle: i32,
}

const INVALID_POLLABLE_HANDLE: i32 = -1;

fn with_incoming_req<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<IncomingRequest>) -> R,
{
    INCOMING_REQ_TABLE.with(|t| f(&mut t.borrow_mut()))
}

fn with_outgoing_req<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<OutgoingRequest>) -> R,
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
/// Called from the export handler.
pub(crate) fn insert_incoming_request(req: IncomingRequest) -> i32 {
    with_incoming_req(|t| t.insert(req))
}

/// Get the HTTP method of an incoming request.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_method(handle: i32, out: *mut HostApiString) {
    with_incoming_req(|t| {
        let req = t.get(handle).expect("invalid incoming request handle");
        let method = req.method();
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
/// Returns a headers handle.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_headers(handle: i32) -> i32 {
    with_incoming_req(|t| {
        let req = t.get(handle).expect("invalid incoming request handle");
        let fields = req.headers();
        // Store in shared headers table
        crate::http_headers::insert_headers(fields)
    })
}

/// Get the body from an incoming request.
/// Returns an incoming body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_body(handle: i32) -> i32 {
    with_incoming_req(|t| {
        let req = t.get(handle).expect("invalid incoming request handle");
        match req.consume() {
            Ok(body) => http_body::insert_incoming_body(body),
            Err(_) => -1,
        }
    })
}

/// Get the scheme of an incoming request.
/// Returns the scheme string in `out`.
#[no_mangle]
pub extern "C" fn host_api_incoming_request_scheme(handle: i32, out: *mut HostApiString) {
    with_incoming_req(|t| {
        let req = t.get(handle).expect("invalid incoming request handle");
        let scheme_str = match req.scheme() {
            Some(Scheme::Http) => "http".to_string(),
            Some(Scheme::Https) => "https".to_string(),
            Some(Scheme::Other(s)) => s,
            None => "".to_string(),
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
        let req = t.get(handle).expect("invalid incoming request handle");
        let auth = req.authority().unwrap_or_default();
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
        let req = t.get(handle).expect("invalid incoming request handle");
        let path = req.path_with_query().unwrap_or_default();
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
/// `method_ptr`/`method_len` is the HTTP method string.
/// `headers_handle` is a handle from the headers table (ownership is taken).
/// `has_url` indicates if URL parts are provided.
/// If has_url is true, then scheme/authority/path are read.
/// Returns an outgoing request handle, or -1 on error.
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

    // Take ownership of the Fields from the headers table.
    let headers =
        crate::http_headers::remove_headers(headers_handle).expect("invalid headers handle");

    let req = OutgoingRequest::new(headers);
    req.set_method(&method).expect("failed to set method");

    if has_url {
        let scheme_str =
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(scheme_ptr, scheme_len));
        let scheme = match scheme_str {
            "http" | "http:" => Scheme::Http,
            "https" | "https:" => Scheme::Https,
            s => Scheme::Other(s.to_string()),
        };
        // Ignore errors from set_scheme/set_authority/set_path_with_query
        // to match old C bindings behavior (non-HTTP schemes like "data:" will
        // fail here, but the request object is still usable).
        let _ = req.set_scheme(Some(&scheme));

        let authority = core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            authority_ptr,
            authority_len,
        ));
        let _ = req.set_authority(Some(authority));

        let path = core::str::from_utf8_unchecked(core::slice::from_raw_parts(path_ptr, path_len));
        let _ = req.set_path_with_query(Some(path));
    }

    with_outgoing_req(|t| t.insert(req))
}

/// Get a headers handle from an outgoing request.
#[no_mangle]
pub extern "C" fn host_api_outgoing_request_headers(handle: i32) -> i32 {
    with_outgoing_req(|t| {
        let req = t.get(handle).expect("invalid outgoing request handle");
        let fields = req.headers();
        crate::http_headers::insert_headers(fields)
    })
}

/// Get the body from an outgoing request.
/// Returns an outgoing body handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_outgoing_request_body(handle: i32) -> i32 {
    with_outgoing_req(|t| {
        let req = t.get(handle).expect("invalid outgoing request handle");
        match req.body() {
            Ok(body) => http_body::insert_outgoing_body(body),
            Err(_) => -1,
        }
    })
}

/// Send an outgoing request. Takes ownership of the request handle.
/// Returns a future incoming response handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_outgoing_request_send(handle: i32) -> i32 {
    let req = with_outgoing_req(|t| t.remove(handle).expect("invalid outgoing request handle"));

    match outgoing_handler::handle(req, None) {
        Ok(future_resp) => with_future_resp(|t| {
            t.insert(FutureResponseState {
                future: future_resp,
                pollable_handle: INVALID_POLLABLE_HANDLE,
            })
        }),
        Err(_) => -1,
    }
}

// === Future Incoming Response ===

/// Subscribe to a future incoming response for readiness.
/// Returns a raw pollable handle.
#[no_mangle]
pub extern "C" fn host_api_future_response_subscribe(handle: i32) -> i32 {
    with_future_resp(|t| {
        let state = t.get_mut(handle).expect("invalid future response handle");
        if state.pollable_handle == INVALID_POLLABLE_HANDLE {
            let pollable = state.future.subscribe();
            let raw = pollable.handle() as i32;
            core::mem::forget(pollable);
            state.pollable_handle = raw;
        }
        state.pollable_handle
    })
}

/// Unsubscribe from a future incoming response's pollable.
#[no_mangle]
pub extern "C" fn host_api_future_response_unsubscribe(handle: i32) {
    with_future_resp(|t| {
        let state = t.get_mut(handle).expect("invalid future response handle");
        if state.pollable_handle != INVALID_POLLABLE_HANDLE {
            let pollable =
                unsafe { wasi::io::poll::Pollable::from_handle(state.pollable_handle as u32) };
            drop(pollable);
            state.pollable_handle = INVALID_POLLABLE_HANDLE;
        }
    });
}

/// Try to get the response from a future.
/// Returns: handle >= 0 means response ready (incoming response handle),
///          -1 means not ready yet, -2 means error.
#[no_mangle]
pub extern "C" fn host_api_future_response_get(handle: i32) -> i32 {
    with_future_resp(|t| {
        let state = t.get(handle).expect("invalid future response handle");
        match state.future.get() {
            None => -1, // not ready
            Some(Ok(Ok(resp))) => {
                // Store the incoming response in the response table
                crate::http_response::insert_incoming_response(resp)
            }
            Some(_) => -2, // error
        }
    })
}

/// Drop a future incoming response handle.
#[no_mangle]
pub extern "C" fn host_api_future_response_drop(handle: i32) {
    with_future_resp(|t| {
        if let Some(state) = t.remove(handle) {
            if state.pollable_handle != INVALID_POLLABLE_HANDLE {
                let pollable =
                    unsafe { wasi::io::poll::Pollable::from_handle(state.pollable_handle as u32) };
                drop(pollable);
            }
        }
    });
}
