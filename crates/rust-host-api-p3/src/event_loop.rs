//! Async event loop driver for WASIp3.
//!
//! Rust owns the task queue and interest tracking (via `task_queue` module).
//! The C++ side only provides callbacks for SpiderMonkey operations:
//! run microtasks, check exceptions, and execute individual tasks.
//!
//! During I/O waits the C++ call stack is completely empty — the Rust async
//! fn is suspended at an `.await` point, returning control to wasmtime's
//! async executor.

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::vec::Vec;

use futures::future::select_all;
use wasip3::clocks::monotonic_clock;

use crate::http_request::PendingResponse;
use crate::poll::WaiterKind;
use crate::task_queue;

// ── C++ callbacks (Rust → C++) ─────────────────────────────────────

extern "C" {
    fn starling_event_loop_set_engine(engine: *mut ());
    pub(crate) fn starling_event_loop_get_engine() -> *mut ();
    fn starling_event_loop_run_microtasks();
    fn starling_event_loop_has_exception() -> bool;
    /// Find a task by task_id in the C++ GC-traced vector, remove and run it.
    /// Returns 1 on success, 0 if task run failed, -1 if not found.
    fn starling_run_task(task_id: i32) -> i32;
}

// ── Public async API ───────────────────────────────────────────────

/// Run the event loop until this request's response is ready or an error occurs.
pub(crate) async fn run(engine: *mut (), request_handle: i32) -> bool {
    unsafe { starling_event_loop_set_engine(engine) };
    task_queue::set_current_request(request_handle);

    loop {
        unsafe { starling_event_loop_run_microtasks() };

        if unsafe { starling_event_loop_has_exception() } {
            return false;
        }

        let has_resp = crate::http_response::has_pending_response(request_handle);
        let req_interest_done = task_queue::request_interest_complete(request_handle);

        if has_resp && req_interest_done {
            return true;
        }

        // TODO: check if this drops warnings on the floor about requests with no responses.
        if task_queue::interest_complete() {
            return true;
        }

        let count = task_queue::task_count_for(request_handle);
        if count == 0 {
            // TODO: this can't happen, given the same check a few lines up. Remove this branch and instead ensure the event loop is only entered if there's at least one task registered for this request.
            if has_resp && req_interest_done {
                return true;
            }
            if !req_interest_done {
                if task_queue::has_other_interest(request_handle) {
                    // TODO: remove the short sleep here if at all possible.
                    monotonic_clock::wait_for(1).await;
                    task_queue::set_current_request(request_handle);
                    continue;
                }
                return false;
            }
            return has_resp;
        }

        if let Some(task_id) = find_immediately_ready(request_handle) {
            task_queue::remove_task(task_id);
            if !run_task(task_id) {
                return false;
            }
            continue;
        }

        let ready_task_id = await_any_task(request_handle).await;
        task_queue::set_current_request(request_handle);
        if ready_task_id >= 0 {
            task_queue::remove_task(ready_task_id);
            if !run_task(ready_task_id) {
                return false;
            }
        }
    }
}

/// Run a task by task_id on the C++ side.
fn run_task(task_id: i32) -> bool {
    let result = unsafe { starling_run_task(task_id) };
    match result {
        1 => true,
        0 => false,
        _ => true, // not found — consumed by concurrent loop
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Scan the task queue for the first immediately-runnable task.
fn find_immediately_ready(request_handle: i32) -> Option<i32> {
    let now = monotonic_clock::now();
    task_queue::find_task_for(request_handle, |task| {
        let handle = task.waiter_handle;

        if handle == -2 {
            return Some(task.task_id);
        }

        if let Some(kind) = crate::poll::get_waiter_kind(handle) {
            match kind {
                WaiterKind::Timer { deadline } if now >= deadline => Some(task.task_id),
                WaiterKind::OutgoingBodyReady { .. } => Some(task.task_id),
                WaiterKind::IncomingBodyReady { body_handle } => {
                    if crate::http_body::has_incoming_data(body_handle) {
                        Some(task.task_id)
                    } else {
                        None
                    }
                }
                WaiterKind::FutureResponseReady { handle: fh } => {
                    if crate::http_request::future_response_is_ready(fh) {
                        Some(task.task_id)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            // No registered waiter kind — treat as immediately ready.
            Some(task.task_id)
        }
    })
}

/// Guard that wraps an in-flight send future. If dropped before the inner
/// future completes (another future won in `select_all`), the send future
/// is restored into `FutureResponseState` so it can be re-polled next
/// iteration instead of being cancelled.
struct SendFutureGuard {
    /// Future-response handle (identifies the FutureResponseState slot).
    fh: i32,
    /// Task id to return when the send completes.
    task_id: i32,
    /// The send future; `None` after it completes successfully.
    future: Option<PendingResponse>,
}

impl Future for SendFutureGuard {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<i32> {
        let this = self.get_mut();
        match this.future.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Ready(result) => {
                this.future = None;
                crate::http_request::complete_send(this.fh, result);
                Poll::Ready(this.task_id)
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

/// Guard that wraps an in-flight body prefetch future. If dropped before
/// the read completes (another future won in `select_all`), the prefetch
/// future is saved for re-polling on the next iteration instead of being
/// cancelled — cancelling a WASI stream-read subtask corrupts wasmtime's
/// handle table.
struct PrefetchGuard {
    body_handle: i32,
    task_id: i32,
    future: Option<crate::http_body::PrefetchFuture>,
}

impl Future for PrefetchGuard {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<i32> {
        let this = self.get_mut();
        match this.future.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Ready(_) => {
                this.future = None;
                Poll::Ready(this.task_id)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for PrefetchGuard {
    fn drop(&mut self) {
        if let Some(fut) = self.future.take() {
            crate::http_body::save_prefetch_future(self.body_handle, fut);
        }
    }
}

/// Await whichever pending task becomes ready first.
///
/// Takes a snapshot of the Rust task queue and builds futures for each.
/// Timer futures are coalesced into a single `wait_until(min_deadline)`.
/// Send futures are wrapped in `SendFutureGuard` for cancellation safety.
///
/// Returns the task_id of the first task to become ready, or -1 to re-check.
async fn await_any_task(request_handle: i32) -> i32 {
    let tasks = task_queue::snapshot_tasks_for(request_handle);
    let mut futs: Vec<Pin<Box<dyn Future<Output = i32>>>> = Vec::new();
    let mut min_timer_deadline: Option<u64> = None;

    for snap in &tasks {
        if let Some(ref kind) = snap.waiter_kind {
            match kind {
                WaiterKind::Timer { deadline } => {
                    min_timer_deadline = Some(match min_timer_deadline {
                        Some(prev) => prev.min(*deadline),
                        None => *deadline,
                    });
                }
                WaiterKind::OutgoingBodyReady { .. } => {
                    return snap.task_id;
                }
                WaiterKind::IncomingBodyReady { body_handle } => {
                    if crate::http_body::has_incoming_data(*body_handle) {
                        return snap.task_id;
                    }
                    let body_handle = *body_handle;
                    let task_id = snap.task_id;
                    let prefetch_fut = crate::http_body::take_prefetch_future(body_handle)
                        .unwrap_or_else(|| {
                            Box::pin(crate::http_body::prefetch_incoming_body(body_handle))
                        });
                    futs.push(Box::pin(PrefetchGuard {
                        body_handle,
                        task_id,
                        future: Some(prefetch_fut),
                    }));
                }
                WaiterKind::FutureResponseReady { handle: fh } => {
                    let fh = *fh;
                    if crate::http_request::future_response_is_ready(fh) {
                        return snap.task_id;
                    }
                    // Wrap in SendFutureGuard so the future is restored (not
                    // cancelled) if another future wins the select_all race.
                    if let Some(send_fut) = crate::http_request::take_send_future(fh) {
                        futs.push(Box::pin(SendFutureGuard {
                            fh,
                            task_id: snap.task_id,
                            future: Some(send_fut),
                        }));
                    }
                }
            }
        } else {
            return snap.task_id;
        }
    }

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
