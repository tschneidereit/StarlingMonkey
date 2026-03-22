//! Task queue and interest tracking, owned by Rust.
//!
//! C++ builtins register tasks via FFI; Rust drives scheduling with direct
//! access to this data — no round-trip FFI for scheduling decisions.
//!
//! Tasks are tagged with the request_handle they belong to. Each concurrent
//! event loop filters to only process its own request's tasks. This avoids
//! the need for push/pop context and works with arbitrary async interleaving.
//!
//! The actual `AsyncTask` C++ objects remain on the C++ side for GC tracing;
//! a matching `task_id` links the two sides.

use core::cell::RefCell;
use std::collections::BTreeMap;
use std::vec::Vec;

use crate::poll::WaiterKind;

/// Metadata for one pending async task.
#[derive(Debug)]
pub(crate) struct TaskEntry {
    /// Unique id for this task (same value used on the C++ side).
    pub task_id: i32,
    /// The waiter handle from `poll.rs` (maps to a `WaiterKind`).
    pub waiter_handle: i32,
    /// The incoming event this task belongs to.
    pub incoming_event_handle: i32,
}

/// Interest tracking state.
#[derive(Debug)]
struct InterestState {
    /// Global interest count.
    global: i32,
    /// Per-incoming-event interest counts.
    per_incoming_event: BTreeMap<i32, i32>,
    /// The incoming event handle currently being processed (for attributing inc/dec).
    current_incoming_event: i32,
}

struct TaskQueueState {
    /// All tasks from all concurrent incoming events, tagged by incoming_event_handle.
    tasks: Vec<TaskEntry>,
    next_id: i32,
    interest: InterestState,
}

thread_local! {
    static STATE: RefCell<TaskQueueState> = const { RefCell::new(TaskQueueState {
        tasks: Vec::new(),
        next_id: 0,
        interest: InterestState {
            global: 0,
            per_incoming_event: BTreeMap::new(),
            current_incoming_event: -1,
        },
    }) };
}

fn with_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut TaskQueueState) -> R,
{
    STATE.with(|s| f(&mut s.borrow_mut()))
}

// ── Task queue operations ──────────────────────────────────────────

/// Register a new task, tagged with the current incoming event. Returns a unique task_id.
pub(crate) fn register_task(waiter_handle: i32) -> i32 {
    with_state(|s| {
        let id = s.next_id;
        s.next_id += 1;
        s.tasks.push(TaskEntry {
            task_id: id,
            waiter_handle,
            incoming_event_handle: s.interest.current_incoming_event,
        });
        id
    })
}

/// Cancel (remove) a task by task_id. Returns true if found.
pub(crate) fn cancel_task(task_id: i32) -> bool {
    with_state(|s| {
        if let Some(pos) = s.tasks.iter().position(|t| t.task_id == task_id) {
            s.tasks.remove(pos);
            true
        } else {
            false
        }
    })
}

/// Remove and return a task by task_id.
pub(crate) fn remove_task(task_id: i32) -> Option<TaskEntry> {
    with_state(|s| {
        if let Some(pos) = s.tasks.iter().position(|t| t.task_id == task_id) {
            Some(s.tasks.remove(pos))
        } else {
            None
        }
    })
}

/// Number of pending tasks for a specific incoming event.
pub(crate) fn task_count_for(incoming_event_handle: i32) -> usize {
    with_state(|s| {
        s.tasks
            .iter()
            .filter(|t| t.incoming_event_handle == incoming_event_handle)
            .count()
    })
}

/// Iterate tasks for a specific incoming event, calling `f` for each.
/// Returns early with `Some(R)` if `f` returns `Some`.
pub(crate) fn find_task_for<F, R>(incoming_event_handle: i32, f: F) -> Option<R>
where
    F: Fn(&TaskEntry) -> Option<R>,
{
    with_state(|s| {
        for task in s.tasks.iter().filter(|t| t.incoming_event_handle == incoming_event_handle) {
            if let Some(r) = f(task) {
                return Some(r);
            }
        }
        None
    })
}

/// Collect task info for a specific incoming event (snapshot to avoid holding borrow).
pub(crate) struct TaskSnapshot {
    pub task_id: i32,
    pub waiter_kind: Option<WaiterKind>,
}

pub(crate) fn snapshot_tasks_for(incoming_event_handle: i32) -> Vec<TaskSnapshot> {
    with_state(|s| {
        s.tasks
            .iter()
            .filter(|t| t.incoming_event_handle == incoming_event_handle)
            .map(|t| {
                let kind = crate::poll::get_waiter_kind(t.waiter_handle);
                TaskSnapshot {
                    task_id: t.task_id,
                    waiter_kind: kind,
                }
            })
            .collect()
    })
}

// ── Interest tracking ──────────────────────────────────────────────

pub(crate) fn set_current_incoming_event(handle: i32) {
    with_state(|s| {
        s.interest.current_incoming_event = handle;
    });
}

pub(crate) fn incr_interest() {
    with_state(|s| {
        s.interest.global += 1;
        let handle = s.interest.current_incoming_event;
        if handle >= 0 {
            *s.interest.per_incoming_event.entry(handle).or_insert(0) += 1;
        }
    });
}

pub(crate) fn decr_interest() {
    with_state(|s| {
        assert!(s.interest.global > 0, "interest underflow");
        s.interest.global -= 1;
        let handle = s.interest.current_incoming_event;
        if handle >= 0 {
            if let Some(count) = s.interest.per_incoming_event.get_mut(&handle) {
                *count -= 1;
                if *count <= 0 {
                    s.interest.per_incoming_event.remove(&handle);
                }
            }
        }
    });
}

pub(crate) fn interest_complete() -> bool {
    with_state(|s| s.interest.global == 0)
}

pub(crate) fn incoming_event_interest_complete(incoming_event_handle: i32) -> bool {
    with_state(|s| {
        s.interest
            .per_incoming_event
            .get(&incoming_event_handle)
            .is_none_or(|&c| c <= 0)
    })
}

pub(crate) fn has_other_interest(incoming_event_handle: i32) -> bool {
    with_state(|s| {
        s.interest
            .per_incoming_event
            .iter()
            .any(|(&h, &c)| h != incoming_event_handle && c > 0)
    })
}

// ── FFI exports (C++ → Rust) ───────────────────────────────────────

/// Register a task. Returns a unique task_id for the C++ side to store.
#[no_mangle]
pub extern "C" fn host_api_register_task(waiter_handle: i32) -> i32 {
    register_task(waiter_handle)
}

/// Cancel a task by task_id.
#[no_mangle]
pub extern "C" fn host_api_cancel_task(task_id: i32) {
    cancel_task(task_id);
}

/// Increment interest (attributed to the current incoming event).
#[no_mangle]
pub extern "C" fn host_api_incr_interest() {
    incr_interest();
}

/// Decrement interest (attributed to the current incoming event).
#[no_mangle]
pub extern "C" fn host_api_decr_interest() {
    decr_interest();
}

/// Set the current incoming event handle for interest attribution.
#[no_mangle]
pub extern "C" fn host_api_set_current_incoming_event(handle: i32) {
    set_current_incoming_event(handle);
}
