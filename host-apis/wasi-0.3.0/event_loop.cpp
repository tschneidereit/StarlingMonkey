/**
 * WASIp3 event loop implementation — async trampoline model.
 *
 * In WASIp3, the event loop is driven by Rust async code. The C++ side
 * exposes its task queue as extern "C" functions that Rust calls:
 *
 *   1. Rust calls starling_event_loop_run_sync() to process all immediately
 *      runnable work (microtasks + ready tasks).
 *   2. C++ returns a status code: done, error, or need-async-wait.
 *   3. If need-async-wait, Rust reads the pending task handles/deadlines
 *      via starling_event_loop_get_tasks().
 *   4. Rust `.await`s the appropriate p3 futures (timers, streams, etc.)
 *      — the C++ call stack is completely empty during this wait.
 *   5. When a future resolves, Rust calls starling_event_loop_complete_task()
 *      to run the ready task.
 *   6. Goto 1.
 *
 * EventLoop::run_event_loop is NOT used for p3; the Rust async handler
 * drives the loop directly. It's kept as a stub that always returns false.
 */
#include "event_loop.h"

#include "extension-api.h"
#include "host_api.h"
#include "jsapi.h"
#include "jsfriendapi.h"

#include <iostream>
#include <unordered_map>
#include <vector>

struct TaskQueue {
  std::vector<RefPtr<api::AsyncTask>> tasks;
  int interest_cnt = 0;
  bool event_loop_running = false;

  // Per-request interest tracking for concurrent request handling.
  // Key: request handle from begin_incoming_request.
  // Value: interest count for that specific request.
  std::unordered_map<int32_t, int> per_request_interest;
  int32_t current_request_handle = -1;

  // Context stack for concurrent request handling.
  // When a nested request starts (e.g. self-referential fetch),
  // the current tasks + interest count are saved and cleared.
  // When the nested request finishes, they're restored.
  struct SavedContext {
    std::vector<RefPtr<api::AsyncTask>> tasks;
    int interest_cnt;
  };
  std::vector<SavedContext> context_stack;

  void trace(JSTracer *trc) const {
    for (const auto &task : tasks) {
      task->trace(trc);
    }
    for (const auto &ctx : context_stack) {
      for (const auto &task : ctx.tasks) {
        task->trace(trc);
      }
    }
  }
};

static PersistentRooted<TaskQueue> queue;
static api::Engine *EVENT_LOOP_ENGINE = nullptr;

namespace core {

void EventLoop::queue_async_task(const RefPtr<api::AsyncTask>& task) {
  MOZ_ASSERT(task);
  queue.get().tasks.emplace_back(task);
}

bool EventLoop::cancel_async_task(api::Engine *engine, const RefPtr<api::AsyncTask>& task) {
  auto *const tasks = &queue.get().tasks;
  for (auto it = tasks->begin(); it != tasks->end(); ++it) {
    if (*it == task) {
      tasks->erase(it);
      task->cancel(engine);
      return true;
    }
  }
  return false;
}

bool EventLoop::has_pending_async_tasks() { return !queue.get().tasks.empty(); }

void EventLoop::incr_event_loop_interest() {
  queue.get().interest_cnt++;
  auto handle = queue.get().current_request_handle;
  if (handle >= 0) {
    queue.get().per_request_interest[handle]++;
  }
}

void EventLoop::decr_event_loop_interest() {
  MOZ_ASSERT(queue.get().interest_cnt > 0);
  queue.get().interest_cnt--;
  auto handle = queue.get().current_request_handle;
  if (handle >= 0) {
    auto it = queue.get().per_request_interest.find(handle);
    if (it != queue.get().per_request_interest.end()) {
      it->second--;
      if (it->second <= 0) {
        queue.get().per_request_interest.erase(it);
      }
    }
  }
}

// Synchronous fallback for wizer pre-initialization.
// During runtime the Rust async handler drives the loop instead.
bool EventLoop::run_event_loop(api::Engine *engine, double total_compute) {
  EVENT_LOOP_ENGINE = engine;
  queue.get().event_loop_running = true;

  while (true) {
    js::RunJobs(engine->cx());

    if (JS_IsExceptionPending(engine->cx())) {
      queue.get().event_loop_running = false;
      return false;
    }

    if (queue.get().interest_cnt == 0) {
      queue.get().event_loop_running = false;
      return true;
    }

    auto *tasks = &queue.get().tasks;
    if (tasks->empty()) {
      queue.get().event_loop_running = false;
      return true;
    }

    // Run the first immediately-ready task (IMMEDIATE_TASK_HANDLE or expired timer).
    bool found = false;
    for (size_t i = 0; i < tasks->size(); i++) {
      if (tasks->at(i)->id() == IMMEDIATE_TASK_HANDLE) {
        auto task = tasks->at(i);
        tasks->erase(tasks->begin() + i);
        task->run(engine);
        found = true;
        break;
      }
    }

    if (!found) {
      // No immediately-ready tasks; async tasks require the Rust event loop.
      queue.get().event_loop_running = false;
      return false;
    }
  }
}

void EventLoop::init(JSContext *cx) { queue.init(cx); }

} // namespace core

// =====================================================================
// Extern "C" interface for the Rust async event loop driver
// =====================================================================

/// Store the engine pointer so task operations can use it.
extern "C" void starling_event_loop_set_engine(void *engine) {
  EVENT_LOOP_ENGINE = static_cast<api::Engine *>(engine);
}

/// Get the engine pointer for Rust to pass back.
extern "C" void *starling_event_loop_get_engine() {
  return static_cast<void *>(api::Engine::get(api::Engine::cx()));
}

/// Run microtasks checkpoint.
extern "C" void starling_event_loop_run_microtasks() {
  MOZ_ASSERT(EVENT_LOOP_ENGINE);
  js::RunJobs(EVENT_LOOP_ENGINE->cx());
}

/// Check if a JS exception is pending.
extern "C" bool starling_event_loop_has_exception() {
  return JS_IsExceptionPending(EVENT_LOOP_ENGINE->cx());
}

/// Check if all event loop interests are satisfied.
extern "C" bool starling_event_loop_interest_complete() {
  return queue.get().interest_cnt == 0;
}

/// Set the current request handle for interest tracking.
extern "C" void starling_event_loop_set_current_request(int32_t request_handle) {
  queue.get().current_request_handle = request_handle;
}

/// Check if a specific request's interest is complete.
extern "C" bool starling_event_loop_request_interest_complete(int32_t request_handle) {
  auto &q = queue.get();
  auto it = q.per_request_interest.find(request_handle);
  return it == q.per_request_interest.end() || it->second <= 0;
}

/// Check if there are other concurrent requests with pending interest.
/// Returns true if at least one request OTHER than the given handle has interest > 0.
extern "C" bool starling_event_loop_has_other_interest(int32_t request_handle) {
  auto &q = queue.get();
  for (auto &[handle, interest] : q.per_request_interest) {
    if (handle != request_handle && interest > 0) {
      return true;
    }
  }
  return false;
}

/// Get the number of pending async tasks.
extern "C" size_t starling_event_loop_task_count() {
  return queue.get().tasks.size();
}

/// Get the pollable/waiter handle for task at index.
extern "C" int32_t starling_event_loop_task_handle(size_t idx) {
  return queue.get().tasks.at(idx)->id();
}

/// Get the deadline for task at index (0 = no deadline).
extern "C" uint64_t starling_event_loop_task_deadline(size_t idx) {
  return queue.get().tasks.at(idx)->deadline();
}

/// Push a new event loop context. Saves current tasks + interest count
/// and starts with an empty task queue. Used for concurrent request handling.
extern "C" void starling_event_loop_push_context() {
  auto &q = queue.get();
  q.context_stack.push_back({std::move(q.tasks), q.interest_cnt});
  q.tasks.clear();
  q.interest_cnt = 0;
}

/// Pop the event loop context, restoring previously saved tasks + interest count.
extern "C" void starling_event_loop_pop_context() {
  auto &q = queue.get();
  MOZ_ASSERT(!q.context_stack.empty());
  auto ctx = std::move(q.context_stack.back());
  q.context_stack.pop_back();
  q.tasks = std::move(ctx.tasks);
  q.interest_cnt = ctx.interest_cnt;
}

/// Run the task at the given index. Removes it from the queue first.
/// Returns true on success, false on error.
extern "C" bool starling_event_loop_run_task(size_t idx) {
  MOZ_ASSERT(EVENT_LOOP_ENGINE);
  auto *tasks = &queue.get().tasks;
  MOZ_ASSERT(idx < tasks->size());

  auto task = tasks->at(idx);
  tasks->erase(tasks->begin() + idx);
  return task->run(EVENT_LOOP_ENGINE);
}

/// Find and run the first task with the given handle.
/// Returns 1 on success, 0 if task run failed, -1 if handle not found.
extern "C" int32_t starling_event_loop_run_task_by_handle(int32_t handle) {
  MOZ_ASSERT(EVENT_LOOP_ENGINE);
  auto *tasks = &queue.get().tasks;
  for (size_t i = 0; i < tasks->size(); i++) {
    if (tasks->at(i)->id() == handle) {
      auto task = tasks->at(i);
      tasks->erase(tasks->begin() + i);
      return task->run(EVENT_LOOP_ENGINE) ? 1 : 0;
    }
  }
  return -1;
}
