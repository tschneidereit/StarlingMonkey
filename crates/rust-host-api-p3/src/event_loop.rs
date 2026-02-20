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
//!   4. Otherwise, `.await` the appropriate p3 future for each pending task
//!      (using a select-any combinator so we return when *any* task is ready)
//!   5. Run the newly-ready task and goto 1

extern crate alloc;

use alloc::vec::Vec;

use wasip3::clocks::monotonic_clock;

use crate::poll::WaiterKind;

// ── C++ callbacks ──────────────────────────────────────────────────────

extern "C" {
    fn starling_event_loop_set_engine(engine: *mut ());
    pub(crate) fn starling_event_loop_get_engine() -> *mut ();
    fn starling_event_loop_run_microtasks();
    fn starling_event_loop_has_exception() -> bool;
    fn starling_event_loop_interest_complete() -> bool;
    fn starling_event_loop_task_count() -> usize;
    fn starling_event_loop_task_handle(idx: usize) -> i32;
    fn starling_event_loop_run_task(idx: usize) -> bool;
}

// ── Public async API ───────────────────────────────────────────────────

/// Run the event loop until all interests are satisfied or an error occurs.
///
/// This is called from the async `handle()` export after the fetch event has
/// been dispatched. Every `.await` point fully unwinds the C++ call stack.
pub(crate) async fn run(engine: *mut ()) -> bool {
    unsafe { starling_event_loop_set_engine(engine) };

    loop {
        unsafe { starling_event_loop_run_microtasks() };

        if unsafe { starling_event_loop_has_exception() } {
            return false;
        }

        if unsafe { starling_event_loop_interest_complete() } {
            return true;
        }

        let task_count = unsafe { starling_event_loop_task_count() };
        if task_count == 0 {
            return false;
        }

        if let Some(idx) = find_immediately_ready(task_count) {
            if !unsafe { starling_event_loop_run_task(idx) } {
                return false;
            }
            continue;
        }

        let ready_idx = await_any_task(task_count).await;
        if !unsafe { starling_event_loop_run_task(ready_idx) } {
            return false;
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Scan the task queue for the first immediately-runnable task.
fn find_immediately_ready(task_count: usize) -> Option<usize> {
    let now = monotonic_clock::now();
    for idx in 0..task_count {
        let handle = unsafe { starling_event_loop_task_handle(idx) };

        if handle == -2 {
            return Some(idx);
        }

        if let Some(kind) = crate::poll::get_waiter_kind(handle) {
            match kind {
                WaiterKind::Timer { deadline } if now >= deadline => return Some(idx),
                WaiterKind::OutgoingBodyReady { .. } => return Some(idx),
                WaiterKind::IncomingBodyReady { body_handle } => {
                    if crate::http_body::has_incoming_data(body_handle) {
                        return Some(idx);
                    }
                }
                WaiterKind::FutureResponseReady { handle } => {
                    if crate::http_request::future_response_is_ready(handle) {
                        return Some(idx);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Await whichever pending task becomes ready first.
///
/// Builds a future for each task and returns the index of the first to complete.
async fn await_any_task(task_count: usize) -> usize {
    // Collect (index, waiter-kind) pairs.
    let mut timer_tasks: Vec<(usize, u64)> = Vec::new();
    let mut incoming_body_tasks: Vec<(usize, i32)> = Vec::new();
    let mut new_sends: Vec<(usize, i32)> = Vec::new();
    let mut inflight_sends: Vec<(usize, i32)> = Vec::new();

    for idx in 0..task_count {
        let handle = unsafe { starling_event_loop_task_handle(idx) };
        if let Some(kind) = crate::poll::get_waiter_kind(handle) {
            match kind {
                WaiterKind::Timer { deadline } => {
                    timer_tasks.push((idx, deadline));
                }
                // Outgoing body writes are buffered — immediately ready.
                WaiterKind::OutgoingBodyReady { .. } => {
                    return idx;
                }
                // Incoming body needs data pre-fetched from the stream.
                WaiterKind::IncomingBodyReady { body_handle } => {
                    incoming_body_tasks.push((idx, body_handle));
                }
                // Future response: check if already in-flight vs new.
                WaiterKind::FutureResponseReady { handle } => {
                    if crate::http_request::is_send_in_flight(handle) {
                        inflight_sends.push((idx, handle));
                    } else if crate::http_request::has_pending_send(handle) {
                        new_sends.push((idx, handle));
                    } else {
                        // No pending and not sending — already complete or error.
                        return idx;
                    }
                }
            }
        } else {
            // Unknown handle — treat as immediately ready to avoid deadlock
            return idx;
        }
    }

    // Determine if we have concurrent work (timers, body reads, or in-flight sends)
    // that should run alongside any new sends.
    let has_concurrent_work = !timer_tasks.is_empty()
        || !incoming_body_tasks.is_empty()
        || !inflight_sends.is_empty();

    // Start any new sends.
    for &(idx, future_handle) in &new_sends {
        if has_concurrent_work || new_sends.len() > 1 {
            // Spawn concurrently so other tasks can proceed.
            crate::http_request::spawn_pending_send(future_handle);
            // Move to in-flight list so we can wait for it below.
            inflight_sends.push((idx, future_handle));
        } else {
            // Only task — await directly.
            crate::http_request::execute_pending_send(future_handle).await;
            return idx;
        }
    }

    // If any incoming body task needs data, pre-fetch the oldest one.
    if let Some(&(idx, body_handle)) = incoming_body_tasks.first() {
        crate::http_body::prefetch_incoming_body(body_handle).await;
        // Check if any in-flight sends also completed during the await.
        for &(send_idx, handle) in &inflight_sends {
            if crate::http_request::future_response_is_ready(handle) {
                return send_idx;
            }
        }
        return idx;
    }

    // Await the timer with the smallest deadline.
    if let Some(&(idx, deadline)) = timer_tasks.iter().min_by_key(|(_, d)| *d) {
        let now = monotonic_clock::now();
        if now < deadline {
            monotonic_clock::wait_until(deadline).await;
        }
        // Check if any in-flight sends also completed.
        for &(send_idx, handle) in &inflight_sends {
            if crate::http_request::future_response_is_ready(handle) {
                return send_idx;
            }
        }
        return idx;
    }

    // Only in-flight sends remain — poll until one completes.
    if !inflight_sends.is_empty() {
        loop {
            for &(idx, handle) in &inflight_sends {
                if crate::http_request::future_response_is_ready(handle) {
                    return idx;
                }
            }
            // Yield to let the executor poll spawned send tasks.
            monotonic_clock::wait_until(monotonic_clock::now() + 1_000_000).await;
        }
    }

    // Shouldn't happen — we already checked task_count > 0
    0
}
