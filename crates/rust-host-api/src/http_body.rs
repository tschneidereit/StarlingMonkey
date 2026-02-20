use crate::handle_table::HandleTable;
use wasi::http::types::{IncomingBody, OutgoingBody};
use wasi::io::streams::{InputStream, OutputStream};

use core::cell::RefCell;

/// State for an incoming body: owns the body, its stream, and optionally a pollable handle.
struct IncomingBodyState {
    _body: IncomingBody,
    stream: InputStream,
}

/// State for an outgoing body: owns the body, its stream, and optionally a pollable handle.
struct OutgoingBodyState {
    body: Option<OutgoingBody>,
    stream: OutputStream,
    pollable_handle: i32,
}

const INVALID_POLLABLE_HANDLE: i32 = -1;

thread_local! {
    static INCOMING_BODY_TABLE: RefCell<HandleTable<IncomingBodyState>> = RefCell::new(HandleTable::new());
    static OUTGOING_BODY_TABLE: RefCell<HandleTable<OutgoingBodyState>> = RefCell::new(HandleTable::new());
}

fn with_incoming<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<IncomingBodyState>) -> R,
{
    INCOMING_BODY_TABLE.with(|t| f(&mut t.borrow_mut()))
}

fn with_outgoing<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<OutgoingBodyState>) -> R,
{
    OUTGOING_BODY_TABLE.with(|t| f(&mut t.borrow_mut()))
}

/// FFI result for reading from an incoming body.
#[repr(C)]
pub struct HostApiReadResult {
    pub ptr: *mut u8,
    pub len: usize,
    pub done: bool,
    pub error: bool,
}

// === Incoming Body ===

/// Create an incoming body handle from a raw WASI IncomingBody handle.
/// This is called from the request/response modules when they extract the body.
pub(crate) fn insert_incoming_body(body: IncomingBody) -> i32 {
    let stream = body.stream().expect("incoming body stream should be available");
    let state = IncomingBodyState {
        _body: body,
        stream,
    };
    with_incoming(|t| t.insert(state))
}

/// Read up to `chunk_size` bytes from an incoming body.
#[no_mangle]
pub extern "C" fn host_api_incoming_body_read(
    handle: i32,
    chunk_size: u32,
    out: *mut HostApiReadResult,
) {
    with_incoming(|t| {
        let state = t.get_mut(handle).expect("invalid incoming body handle");
        // Call the raw WASI ABI directly to avoid the debug-mode panic in
        // Vec::from_raw_parts when the host returns a null pointer for an empty list.
        unsafe {
            #[repr(align(4))]
            struct RetArea([core::mem::MaybeUninit<u8>; 12]);
            let mut ret_area = RetArea([core::mem::MaybeUninit::uninit(); 12]);
            let ptr0 = ret_area.0.as_mut_ptr().cast::<u8>();

            #[cfg(target_arch = "wasm32")]
            #[link(wasm_import_module = "wasi:io/streams@0.2.9")]
            extern "C" {
                #[link_name = "[method]input-stream.read"]
                fn wit_import(handle: i32, len: i64, ret: *mut u8);
            }
            #[cfg(not(target_arch = "wasm32"))]
            unsafe extern "C" fn wit_import(_: i32, _: i64, _: *mut u8) {
                unreachable!()
            }

            wit_import(
                state.stream.handle() as i32,
                chunk_size as i64,
                ptr0,
            );

            let discriminant = *ptr0.cast::<u8>();
            match discriminant {
                0 => {
                    // Success: (ptr, len) at offsets 4 and 8
                    let data_ptr = *ptr0.add(4).cast::<*mut u8>();
                    let data_len = *ptr0.add(8).cast::<usize>();

                    if data_len == 0 {
                        // Empty read — no data available yet but stream not closed
                        *out = HostApiReadResult {
                            ptr: core::ptr::null_mut(),
                            len: 0,
                            done: false,
                            error: false,
                        };
                    } else {
                        // Got data — take ownership of the buffer
                        let boxed = Box::from_raw(core::slice::from_raw_parts_mut(data_ptr, data_len));
                        *out = HostApiReadResult {
                            ptr: Box::into_raw(boxed) as *mut u8,
                            len: data_len,
                            done: false,
                            error: false,
                        };
                    }
                }
                1 => {
                    // Error variant
                    let error_discriminant = *ptr0.add(4).cast::<u8>();
                    if error_discriminant == 1 {
                        // StreamError::Closed
                        *out = HostApiReadResult {
                            ptr: core::ptr::null_mut(),
                            len: 0,
                            done: true,
                            error: false,
                        };
                    } else {
                        // StreamError::LastOperationFailed or other error
                        // Drop the error resource handle to avoid leaking it
                        let error_handle = *ptr0.add(8).cast::<i32>();
                        if error_discriminant == 0 {
                            drop(wasi::io::error::Error::from_handle(error_handle as u32));
                        }
                        *out = HostApiReadResult {
                            ptr: core::ptr::null_mut(),
                            len: 0,
                            done: false,
                            error: true,
                        };
                    }
                }
                _ => {
                    *out = HostApiReadResult {
                        ptr: core::ptr::null_mut(),
                        len: 0,
                        done: false,
                        error: true,
                    };
                }
            }
        }
    });
}

/// Subscribe to an incoming body's stream for readiness notification.
/// Returns a raw pollable handle.
#[no_mangle]
pub extern "C" fn host_api_incoming_body_subscribe(handle: i32) -> i32 {
    with_incoming(|t| {
        let state = t.get_mut(handle).expect("invalid incoming body handle");
        let pollable = state.stream.subscribe();
        let raw = pollable.handle() as i32;
        core::mem::forget(pollable); // Caller manages pollable lifetime via host_api_pollable_drop.
        raw
    })
}

/// Close and drop an incoming body handle.
#[no_mangle]
pub extern "C" fn host_api_incoming_body_close(handle: i32) {
    with_incoming(|t| {
        t.remove(handle);
    });
}

// === Outgoing Body ===

/// Create an outgoing body handle from a raw WASI OutgoingBody handle.
pub(crate) fn insert_outgoing_body(body: OutgoingBody) -> i32 {
    let stream = body.write().expect("outgoing body stream should be available");
    let state = OutgoingBodyState {
        body: Some(body),
        stream,
        pollable_handle: INVALID_POLLABLE_HANDLE,
    };
    with_outgoing(|t| t.insert(state))
}

/// Get the outgoing body stream's current write capacity.
/// Returns the capacity on success, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_capacity(handle: i32) -> i64 {
    with_outgoing(|t| {
        let state = t.get(handle).expect("invalid outgoing body handle");
        match state.stream.check_write() {
            Ok(capacity) => capacity as i64,
            Err(_) => -1,
        }
    })
}

/// Write bytes to an outgoing body.
/// The caller must have checked capacity first.
/// Returns true on success.
#[no_mangle]
pub unsafe extern "C" fn host_api_outgoing_body_write(
    handle: i32,
    bytes: *const u8,
    len: usize,
) -> bool {
    let data = core::slice::from_raw_parts(bytes, len);
    with_outgoing(|t| {
        let state = t.get(handle).expect("invalid outgoing body handle");
        state.stream.write(data).is_ok()
    })
}

/// Subscribe to an outgoing body's stream for write-readiness.
/// Returns a raw pollable handle.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_subscribe(handle: i32) -> i32 {
    with_outgoing(|t| {
        let state = t.get_mut(handle).expect("invalid outgoing body handle");
        if state.pollable_handle == INVALID_POLLABLE_HANDLE {
            let pollable = state.stream.subscribe();
            let raw = pollable.handle() as i32;
            core::mem::forget(pollable);
            state.pollable_handle = raw;
        }
        state.pollable_handle
    })
}

/// Unsubscribe from outgoing body pollable.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_unsubscribe(handle: i32) {
    with_outgoing(|t| {
        let state = t.get_mut(handle).expect("invalid outgoing body handle");
        if state.pollable_handle != INVALID_POLLABLE_HANDLE {
            let pollable =
                unsafe { wasi::io::poll::Pollable::from_handle(state.pollable_handle as u32) };
            drop(pollable);
            state.pollable_handle = INVALID_POLLABLE_HANDLE;
        }
    });
}

/// Close an outgoing body: flush the stream, drop the stream and pollable,
/// then finish the body.
/// Returns true on success.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_close(handle: i32) -> bool {
    with_outgoing(|t| {
        if let Some(mut state) = t.remove(handle) {
            // Blocking flush before finishing.
            let _ = state.stream.blocking_flush();

            // Drop pollable if any.
            if state.pollable_handle != INVALID_POLLABLE_HANDLE {
                let pollable = unsafe {
                    wasi::io::poll::Pollable::from_handle(state.pollable_handle as u32)
                };
                drop(pollable);
            }

            // Drop the stream before finishing the body.
            drop(state.stream);

            // Finish the body (no trailers).
            if let Some(body) = state.body.take() {
                let _ = OutgoingBody::finish(body, None);
            }
            true
        } else {
            false
        }
    })
}
