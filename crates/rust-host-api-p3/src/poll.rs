use crate::handle_table::HandleTable;
use core::cell::RefCell;
use wasip3::clocks::monotonic_clock;

/// The different kinds of async operations we can wait on.
#[derive(Debug)]
pub(crate) enum WaiterKind {
    /// Wait for a monotonic clock deadline.
    Timer { deadline: u64 },
    /// An incoming body stream has data available.
    IncomingBodyReady { body_handle: i32 },
    /// An outgoing body stream is ready for writing.
    OutgoingBodyReady { body_handle: i32 },
    /// A future HTTP response is ready.
    FutureResponseReady { handle: i32 },
}

thread_local! {
    static WAITER_TABLE: RefCell<HandleTable<WaiterKind>> = RefCell::new(HandleTable::new());
}

fn with_waiters<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<WaiterKind>) -> R,
{
    WAITER_TABLE.with(|t| f(&mut t.borrow_mut()))
}

/// Register a timer waiter. Returns a waiter handle.
pub(crate) fn register_timer_waiter(deadline: u64) -> i32 {
    with_waiters(|t| t.insert(WaiterKind::Timer { deadline }))
}

/// Register an incoming body readiness waiter. Returns a waiter handle.
pub(crate) fn register_incoming_body_waiter(body_handle: i32) -> i32 {
    with_waiters(|t| t.insert(WaiterKind::IncomingBodyReady { body_handle }))
}

/// Register an outgoing body readiness waiter. Returns a waiter handle.
pub(crate) fn register_outgoing_body_waiter(body_handle: i32) -> i32 {
    with_waiters(|t| t.insert(WaiterKind::OutgoingBodyReady { body_handle }))
}

/// Register a future response readiness waiter. Returns a waiter handle.
pub(crate) fn register_future_response_waiter(handle: i32) -> i32 {
    with_waiters(|t| t.insert(WaiterKind::FutureResponseReady { handle }))
}

/// Get the waiter kind for a handle.
pub(crate) fn get_waiter_kind(handle: i32) -> Option<WaiterKind> {
    with_waiters(|t| {
        t.get(handle).map(|w| match w {
            WaiterKind::Timer { deadline } => WaiterKind::Timer { deadline: *deadline },
            WaiterKind::IncomingBodyReady { body_handle } => WaiterKind::IncomingBodyReady { body_handle: *body_handle },
            WaiterKind::OutgoingBodyReady { body_handle } => WaiterKind::OutgoingBodyReady { body_handle: *body_handle },
            WaiterKind::FutureResponseReady { handle } => WaiterKind::FutureResponseReady { handle: *handle },
        })
    })
}

/// Poll a list of waiter handles, returning the index of the first ready one.
///
/// In WASIp3, the async event loop handles all blocking. This function only
/// checks for immediate readiness — it does NOT block.
#[no_mangle]
pub unsafe extern "C" fn host_api_poll(handles: *const i32, count: usize) -> usize {
    let handle_slice = core::slice::from_raw_parts(handles, count);

    // Return the smallest ready index (oldest-first scheduling).
    for (idx, &h) in handle_slice.iter().enumerate() {
        if is_immediately_ready(h) {
            return idx;
        }
    }

    // Nothing immediately ready. In p3, the Rust async event loop handles
    // all blocking via `.await`. Return 0 as a fallback.
    0
}

/// Check if a waiter is immediately ready (no blocking needed).
fn is_immediately_ready(handle: i32) -> bool {
    if let Some(kind) = get_waiter_kind(handle) {
        match kind {
            WaiterKind::Timer { deadline } => monotonic_clock::now() >= deadline,
            // In WASIp3, body reads/writes and HTTP sends use block_on()
            // inside their respective FFI functions, which yields to wasmtime.
            // Mark these as always ready so the task runs and blocks there.
            WaiterKind::IncomingBodyReady { .. } => true,
            WaiterKind::OutgoingBodyReady { .. } => true,
            WaiterKind::FutureResponseReady { handle } => {
                crate::http_request::future_response_is_ready(handle)
            }
        }
    } else {
        // Unknown handle — treat as ready to avoid infinite loop
        true
    }
}

/// Block on a single waiter handle until it's ready.
///
/// In WASIp3, the async event loop handles all blocking. This is a no-op.
#[no_mangle]
pub extern "C" fn host_api_pollable_block(_handle: i32) {
    // In p3, blocking is handled by the Rust async event loop via `.await`.
    // This function should not be called during normal operation.
}

/// Drop a waiter handle.
#[no_mangle]
pub extern "C" fn host_api_pollable_drop(handle: i32) {
    with_waiters(|t| {
        t.remove(handle);
    });
}
