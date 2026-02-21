//! Async event loop driver for WASIp3.
//!
//! This module drives the C++ task queue from Rust async code. During I/O waits
//! the C++ call stack is completely empty — the Rust async fn is suspended at an
//! `.await` point, returning control to wasmtime's async executor.
//!
//! Flow:
//!   1. Run JS microtasks (C++ callback)
//!   2. Check for done / error / empty-task conditions
//!   3. Find the first immediately-ready task; if found, run it and goto 1
//!   4. Otherwise, race all pending tasks using `futures::future::select_all`:
//!      timers, body reads, and send-completion signals are all awaited
//!      concurrently so the runtime can wake us as soon as any resolves.
//!   5. Run the newly-ready task and goto 1

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::vec::Vec;

use futures::future::select_all;
use wasip3::clocks::monotonic_clock;

use crate::http_request::PendingResponse;
use crate::poll::WaiterKind;

// ── C++ callbacks ──────────────────────────────────────────────────────

extern "C" {
    fn starling_event_loop_set_engine(engine: *mut ());
    pub(crate) fn starling_event_loop_get_engine() -> *mut ();
    fn starling_event_loop_run_microtasks();
    fn starling_event_loop_has_exception() -> bool;
    fn starling_event_loop_interest_complete() -> bool;
    fn starling_event_loop_request_interest_complete(request_handle: i32) -> bool;
    fn starling_event_loop_has_other_interest(request_handle: i32) -> bool;
    fn starling_event_loop_set_current_request(request_handle: i32);
    fn starling_event_loop_task_count() -> usize;
    fn starling_event_loop_task_handle(idx: usize) -> i32;
    /// Find and run the first task with the given handle.
    /// Returns 1 on success, 0 if task run failed, -1 if handle not found.
    fn starling_event_loop_run_task_by_handle(handle: i32) -> i32;
}

// ── Public async API ───────────────────────────────────────────────────

/// Run the event loop until this request's response is ready or an error occurs.
///
/// This is called from the async `handle()` export after the fetch event has
/// been dispatched. Every `.await` point fully unwinds the C++ call stack.
///
/// `request_handle` identifies which request this event loop is serving.
/// The loop exits with success when a response has been stored for this handle.
pub(crate) async fn run(engine: *mut (), request_handle: i32) -> bool {
    unsafe { starling_event_loop_set_engine(engine) };
    // Ensure the event loop knows which request we're serving
    // so interest tracking is attributed correctly.
    unsafe { starling_event_loop_set_current_request(request_handle) };

    loop {
        unsafe { starling_event_loop_run_microtasks() };

        if unsafe { starling_event_loop_has_exception() } {
            return false;
        }

        let has_resp = crate::http_response::has_pending_response(request_handle);
        let req_interest_done =
            unsafe { starling_event_loop_request_interest_complete(request_handle) };

        // Check if our response is ready AND our per-request interest is complete.
        if has_resp && req_interest_done {
            return true;
        }

        // Fallback: if all global interest is complete (e.g. no handler registered),
        // exit. This handles the case where no respondWith was ever called.
        if unsafe { starling_event_loop_interest_complete() } {
            return true;
        }

        let task_count = unsafe { starling_event_loop_task_count() };
        if task_count == 0 {
            if has_resp && req_interest_done {
                return true;
            }
            if !req_interest_done {
                // No tasks but still have pending interest (e.g. a waitUntil promise
                // waiting for another concurrent request to resolve it).
                // Only yield if there are other concurrent requests that could
                // potentially resolve our interest via cross-request globals.
                if unsafe { starling_event_loop_has_other_interest(request_handle) } {
                    monotonic_clock::wait_for(1).await;
                    unsafe { starling_event_loop_set_current_request(request_handle) };
                    continue;
                }
                // No other requests running — we're truly stalled.
                // Return false to trigger the stall warning.
                return false;
            }
            return has_resp;
        }

        if let Some(handle) = find_immediately_ready(task_count) {
            if !run_task(handle) {
                return false;
            }
            continue;
        }

        let ready_handle = await_any_task(task_count).await;
        // After waking from await, restore our request handle as current
        // so interest changes during microtask processing are attributed correctly.
        unsafe { starling_event_loop_set_current_request(request_handle) };
        // Sentinel -1 means "yield completed, re-check" — no specific task to run.
        if ready_handle >= 0
            && !run_task(ready_handle) {
                return false;
            }
    }
}

/// Run a task identified by its handle. Uses handle-based lookup to avoid
/// stale index issues when concurrent requests modify the shared task queue.
fn run_task(handle: i32) -> bool {
    let result = unsafe { starling_event_loop_run_task_by_handle(handle) };
    match result {
        1 => true,  // success
        0 => false, // task run failed
        _ => {
            // Handle not found — the task was already consumed by another
            // concurrent event loop. This is not an error.
            true
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Scan the task queue for the first immediately-runnable task.
/// Returns the task's handle (not index) for stable identification.
fn find_immediately_ready(task_count: usize) -> Option<i32> {
    let now = monotonic_clock::now();
    for idx in 0..task_count {
        let handle = unsafe { starling_event_loop_task_handle(idx) };

        if handle == -2 {
            return Some(handle);
        }

        if let Some(kind) = crate::poll::get_waiter_kind(handle) {
            match kind {
                WaiterKind::Timer { deadline } if now >= deadline => {
                    return Some(handle);
                }
                WaiterKind::OutgoingBodyReady { .. } => {
                    return Some(handle);
                }
                WaiterKind::IncomingBodyReady { body_handle } => {
                    if crate::http_body::has_incoming_data(body_handle) {
                        return Some(handle);
                    }
                }
                WaiterKind::FutureResponseReady { handle: fh } => {
                    if crate::http_request::future_response_is_ready(fh) {
                        return Some(handle);
                    }
                }
                _ => {}
            }
        } else {
            // Handle has no registered waiter kind. This can happen for
            // special or synthetic handles (e.g. -2 sentinel handled above,
            // or C++ tasks that don't map to a WASI waitable). Treat as
            // immediately ready so the C++ side can process them.
            return Some(handle);
        }
    }
    None
}

/// Guard that wraps an in-flight send future. If dropped before the inner
/// future completes (i.e. another future won in `select_all`), the send
/// future is restored into `FutureResponseState` so it can be re-polled on
/// the next event-loop iteration instead of being cancelled.
struct SendFutureGuard {
    /// Future-response handle (identifies the FutureResponseState slot).
    fh: i32,
    /// Task handle to return when the send completes.
    task_handle: i32,
    /// The send future; `None` after it completes successfully.
    future: Option<PendingResponse>,
}

impl Future for SendFutureGuard {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<i32> {
        let this = self.get_mut();
        match this.future.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Ready(result) => {
                this.future = None; // consumed — Drop will see None
                crate::http_request::complete_send(this.fh, result);
                Poll::Ready(this.task_handle)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for SendFutureGuard {
    fn drop(&mut self) {
        if let Some(fut) = self.future.take() {
            crate::http_request::restore_send_future(self.fh, fut);
        }
    }
}

/// Await whichever pending task becomes ready first.
///
/// Uses `futures::future::select_all` to race all pending operations
/// concurrently. Each task type produces a future backed by a WASI waitable
/// (timer subtask, stream-read subtask, or HTTP send subtask), so the
/// runtime can wake us as soon as any one of them resolves — no polling or
/// sleep-based yielding required.
///
/// Timer futures are coalesced: instead of creating a separate `wait_until`
/// subtask per timer (which causes handle corruption when the losing futures
/// are dropped/cancelled by `select_all`), we find the earliest deadline and
/// await a single `wait_until`. The main loop re-scans for ready timers.
///
/// Send futures are wrapped in `SendFutureGuard` so that if they lose the
/// race, they are restored to state rather than dropped/cancelled.
///
/// Returns the handle of the first task to become ready, or -1 to re-check.
async fn await_any_task(task_count: usize) -> i32 {
    let mut futs: Vec<Pin<Box<dyn Future<Output = i32>>>> = Vec::new();
    let mut min_timer_deadline: Option<u64> = None;

    for idx in 0..task_count {
        let handle = unsafe { starling_event_loop_task_handle(idx) };
        if let Some(kind) = crate::poll::get_waiter_kind(handle) {
            match kind {
                WaiterKind::Timer { deadline } => {
                    // Track the earliest deadline — a single wait_until is
                    // created below to avoid subtask cancellation issues.
                    min_timer_deadline = Some(match min_timer_deadline {
                        Some(prev) => prev.min(deadline),
                        None => deadline,
                    });
                }
                // Outgoing body writes are buffered — immediately ready.
                WaiterKind::OutgoingBodyReady { .. } => {
                    return handle;
                }
                // Incoming body needs data pre-fetched from the stream.
                WaiterKind::IncomingBodyReady { body_handle } => {
                    if crate::http_body::has_incoming_data(body_handle) {
                        return handle;
                    }
                    futs.push(Box::pin(async move {
                        crate::http_body::prefetch_incoming_body(body_handle).await;
                        handle
                    }));
                }
                WaiterKind::FutureResponseReady { handle: fh } => {
                    if crate::http_request::future_response_is_ready(fh) {
                        return handle;
                    }
                    // Wrap in SendFutureGuard so the future is restored (not
                    // cancelled) if another future wins the select_all race.
                    if let Some(send_fut) = crate::http_request::take_send_future(fh) {
                        futs.push(Box::pin(SendFutureGuard {
                            fh,
                            task_handle: handle,
                            future: Some(send_fut),
                        }));
                    }
                }
            }
        } else {
            // No registered waiter — treat as immediately ready so C++ can process it.
            return handle;
        }
    }

    // Coalesce all pending timers into a single wait_until for the earliest
    // deadline. Racing multiple wait_until subtask futures in select_all
    // causes the losing futures to be cancelled via [subtask-cancel] /
    // [subtask-drop], which corrupts wasmtime's handle table. A single
    // future avoids this; the main loop's find_immediately_ready will
    // discover which timer(s) actually expired.
    if let Some(deadline) = min_timer_deadline {
        futs.push(Box::pin(async move {
            monotonic_clock::wait_until(deadline).await;
            -1 // sentinel: re-scan for ready timer(s)
        }));
    }

    if futs.is_empty() {
        return -1;
    }

    let (result, _, _) = select_all(futs).await;
    result
}
