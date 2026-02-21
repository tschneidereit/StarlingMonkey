use crate::handle_table::HandleTable;
use core::cell::RefCell;

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
    static WAITER_TABLE: RefCell<HandleTable<WaiterKind>> = const { RefCell::new(HandleTable::new()) };
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
            WaiterKind::Timer { deadline } => WaiterKind::Timer {
                deadline: *deadline,
            },
            WaiterKind::IncomingBodyReady { body_handle } => WaiterKind::IncomingBodyReady {
                body_handle: *body_handle,
            },
            WaiterKind::OutgoingBodyReady { body_handle } => WaiterKind::OutgoingBodyReady {
                body_handle: *body_handle,
            },
            WaiterKind::FutureResponseReady { handle } => {
                WaiterKind::FutureResponseReady { handle: *handle }
            }
        })
    })
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
