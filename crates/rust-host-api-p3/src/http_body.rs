extern crate alloc;

use alloc::vec::Vec;
use crate::handle_table::HandleTable;
use wit_bindgen::rt::async_support::{StreamReader, StreamWriter};

use core::cell::RefCell;

/// State for an incoming body (response or request body being read).
/// In WASIp3, the body is a StreamReader<u8>. Data is pre-fetched by the
/// async event loop and stored in `read_buffer` for synchronous consumption
/// by C++ tasks.
struct IncomingBodyState {
    reader: Option<StreamReader<u8>>,
    /// Pre-fetched data from the stream, populated by the event loop.
    read_buffer: Vec<u8>,
    /// Whether the stream has reached EOF.
    eof: bool,
}

/// State for an outgoing body (request or response body being written).
/// In WASIp3, writes are buffered in `write_buffer` and flushed
/// asynchronously after `task.return` via a spawned task.
struct OutgoingBodyState {
    writer: Option<StreamWriter<u8>>,
    /// Buffered data to be written to the stream after the Response is returned.
    write_buffer: Vec<u8>,
}

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

/// Create an incoming body handle from a StreamReader<u8>.
pub(crate) fn insert_incoming_body(reader: StreamReader<u8>) -> i32 {
    let state = IncomingBodyState {
        reader: Some(reader),
        read_buffer: Vec::new(),
        eof: false,
    };
    with_incoming(|t| t.insert(state))
}

/// Pre-fetch data from an incoming body stream.
///
/// Called by the event loop before running the C++ task that will read the
/// body. This `.await`s the stream reader, storing the result for the
/// subsequent synchronous `host_api_incoming_body_read` call.
pub(crate) async fn prefetch_incoming_body(body_handle: i32) -> bool {
    // Take the reader out to avoid borrowing across await.
    let reader_opt = with_incoming(|t| {
        t.get_mut(body_handle).and_then(|s| s.reader.take())
    });

    if let Some(mut reader) = reader_opt {
        // Read up to 4096 bytes at once for efficiency.
        let buf = Vec::with_capacity(4096);
        let (_result, data) = reader.read(buf).await;
        if data.is_empty() {
            // Stream ended (EOF).
            with_incoming(|t| {
                if let Some(state) = t.get_mut(body_handle) {
                    state.eof = true;
                }
            });
        } else {
            with_incoming(|t| {
                if let Some(state) = t.get_mut(body_handle) {
                    state.read_buffer.extend_from_slice(&data);
                    state.reader = Some(reader);
                }
            });
        }
        true
    } else {
        true // No reader or already consumed
    }
}

/// Check if an incoming body has pre-fetched data available or has reached EOF.
pub(crate) fn has_incoming_data(body_handle: i32) -> bool {
    with_incoming(|t| {
        t.get(body_handle)
            .map_or(true, |s| !s.read_buffer.is_empty() || s.eof)
    })
}

/// Read from an incoming body's pre-fetched buffer.
///
/// The event loop must call `prefetch_incoming_body()` before the C++ task
/// runs, so data is available in the buffer when this is called.
///
/// When `chunk_size` is 0, reports EOF status without consuming any data.
#[no_mangle]
pub extern "C" fn host_api_incoming_body_read(
    handle: i32,
    chunk_size: u32,
    out: *mut HostApiReadResult,
) {
    with_incoming(|t| {
        let state = t.get_mut(handle).expect("invalid incoming body handle");

        // chunk_size 0 is a state probe: report done/not-done without consuming data.
        if chunk_size == 0 {
            unsafe {
                *out = HostApiReadResult {
                    ptr: core::ptr::null_mut(),
                    len: 0,
                    done: state.eof && state.read_buffer.is_empty(),
                    error: false,
                };
            }
            return;
        }

        if !state.read_buffer.is_empty() {
            let bytes = core::mem::take(&mut state.read_buffer);
            let mut boxed = bytes.into_boxed_slice();
            let ptr = boxed.as_mut_ptr();
            let len = boxed.len();
            core::mem::forget(boxed);
            unsafe {
                *out = HostApiReadResult {
                    ptr,
                    len,
                    done: false,
                    error: false,
                };
            }
        } else if state.eof {
            unsafe {
                *out = HostApiReadResult {
                    ptr: core::ptr::null_mut(),
                    len: 0,
                    done: true,
                    error: false,
                };
            }
        } else {
            // No data in buffer and not EOF. Should not happen if the event
            // loop properly pre-fetched. Return empty to avoid blocking.
            unsafe {
                *out = HostApiReadResult {
                    ptr: core::ptr::null_mut(),
                    len: 0,
                    done: false,
                    error: false,
                };
            }
        }
    });
}

/// Subscribe to an incoming body for readiness notification.
/// Returns a waiter handle.
#[no_mangle]
pub extern "C" fn host_api_incoming_body_subscribe(handle: i32) -> i32 {
    crate::poll::register_incoming_body_waiter(handle)
}

/// Close and drop an incoming body handle.
#[no_mangle]
pub extern "C" fn host_api_incoming_body_close(handle: i32) {
    with_incoming(|t| {
        t.remove(handle);
    });
}

// === Outgoing Body ===

/// Create an outgoing body handle from a StreamWriter<u8>.
pub(crate) fn insert_outgoing_body(writer: StreamWriter<u8>) -> i32 {
    let state = OutgoingBodyState {
        writer: Some(writer),
        write_buffer: Vec::new(),
    };
    with_outgoing(|t| t.insert(state))
}

/// Get the outgoing body stream's current write capacity.
///
/// Since writes are buffered in-process, capacity is always available.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_capacity(handle: i32) -> i64 {
    with_outgoing(|t| {
        let state = t.get(handle).expect("invalid outgoing body handle");
        if state.writer.is_some() {
            4096
        } else {
            -1
        }
    })
}

/// Buffer bytes for later writing to an outgoing body stream.
///
/// The actual stream write happens asynchronously after `task.return`,
/// via the spawned flush task in exports.rs.
#[no_mangle]
pub unsafe extern "C" fn host_api_outgoing_body_write(
    handle: i32,
    bytes: *const u8,
    len: usize,
) -> bool {
    let data = core::slice::from_raw_parts(bytes, len);
    with_outgoing(|t| {
        if let Some(state) = t.get_mut(handle) {
            state.write_buffer.extend_from_slice(data);
            true
        } else {
            false
        }
    })
}

/// Subscribe to an outgoing body for write-readiness.
/// Returns a waiter handle.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_subscribe(handle: i32) -> i32 {
    crate::poll::register_outgoing_body_waiter(handle)
}

/// Unsubscribe from outgoing body waiter.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_unsubscribe(_handle: i32) {
    // Waiter cleanup happens when the body is closed.
}

/// Close an outgoing body, spawning an async flush of any buffered data.
///
/// The spawned task writes the buffered data to the p3 stream and then drops
/// the writer (closing the stream). The write completes when the reader
/// consumes the data — for response bodies that happens after task.return;
/// for request bodies it happens concurrently with `client::send().await`.
#[no_mangle]
pub extern "C" fn host_api_outgoing_body_close(handle: i32) -> bool {
    with_outgoing(|t| {
        if let Some(state) = t.remove(handle) {
            if let Some(writer) = state.writer {
                let buffer = state.write_buffer;
                wit_bindgen::rt::async_support::spawn(async move {
                    let mut w = writer;
                    if !buffer.is_empty() {
                        w.write_all(buffer).await;
                    }
                    // Dropping w closes the stream.
                });
            }
            true
        } else {
            false
        }
    })
}
