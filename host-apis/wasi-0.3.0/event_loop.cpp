/**
 * WASIp3 event loop — C++ side.
 *
 * The Rust async event loop owns task scheduling and interest tracking.
 * This C++ file maintains a GC-traced vector of AsyncTask objects and
 * provides extern "C" callbacks that Rust calls to run tasks and
 * microtasks.
 *
 * Task registration: C++ → Rust via host_api_register_task()
 * Task execution:    Rust → C++ via starling_run_task()
 * Interest:          C++ → Rust via host_api_incr/decr_interest()
 */
#include "event_loop.h"

#include "extension-api.h"
#include "host_api.h"
#include "jsapi.h"
#include "jsfriendapi.h"

#include <vector>

// ── Rust FFI (C++ → Rust) ──────────────────────────────────────────
extern "C" {
int32_t host_api_register_task(int32_t waiter_handle);
void host_api_cancel_task(int32_t task_id);
void host_api_incr_interest();
void host_api_decr_interest();
void host_api_set_current_request(int32_t handle);
}

// ── GC-traced task storage ─────────────────────────────────────────

struct TaskQueue {
  /// Pairs of (task_id, AsyncTask). The task_id links to the Rust task queue.
  std::vector<std::pair<int32_t, RefPtr<api::AsyncTask>>> tasks;

  void trace(JSTracer *trc) const {
    for (const auto &[id, task] : tasks) {
      task->trace(trc);
    }
  }
};

static PersistentRooted<TaskQueue> queue;
static api::Engine *EVENT_LOOP_ENGINE = nullptr;

// ── EventLoop namespace (called by builtins / engine) ──────────────

namespace core {

void EventLoop::queue_async_task(const RefPtr<api::AsyncTask>& task) {
  MOZ_ASSERT(task);
  auto waiter_handle = task->id();
  auto task_id = host_api_register_task(waiter_handle);
  task->set_task_id(task_id);
  queue.get().tasks.emplace_back(task_id, task);
}

bool EventLoop::cancel_async_task(api::Engine *engine, const RefPtr<api::AsyncTask>& task) {
  auto &tasks = queue.get().tasks;
  for (auto it = tasks.begin(); it != tasks.end(); ++it) {
    if (it->second == task) {
      auto task_id = it->first;
      tasks.erase(it);
      host_api_cancel_task(task_id);
      task->cancel(engine);
      return true;
    }
  }
  return false;
}

bool EventLoop::has_pending_async_tasks() { return !queue.get().tasks.empty(); }

void EventLoop::incr_event_loop_interest() {
  host_api_incr_interest();
}

void EventLoop::decr_event_loop_interest() {
  host_api_decr_interest();
}

// Synchronous fallback for wizer pre-initialization only.
bool EventLoop::run_event_loop(api::Engine *engine, double total_compute) {
  EVENT_LOOP_ENGINE = engine;

  while (true) {
    js::RunJobs(engine->cx());

    if (JS_IsExceptionPending(engine->cx())) {
      return false;
    }

    // During wizer, only IMMEDIATE_TASK_HANDLE tasks are created.
    auto &tasks = queue.get().tasks;
    if (tasks.empty()) {
      return true;
    }

    bool found = false;
    for (size_t i = 0; i < tasks.size(); i++) {
      if (tasks[i].second->id() == IMMEDIATE_TASK_HANDLE) {
        auto task = tasks[i].second;
        auto task_id = tasks[i].first;
        tasks.erase(tasks.begin() + i);
        host_api_cancel_task(task_id);
        task->run(engine);
        found = true;
        break;
      }
    }

    if (!found) {
      return false;
    }
  }
}

void EventLoop::init(JSContext *cx) { queue.init(cx); }

} // namespace core

// =====================================================================
// Extern "C" interface for Rust async event loop driver
// =====================================================================

extern "C" void starling_event_loop_set_engine(void *engine) {
  EVENT_LOOP_ENGINE = static_cast<api::Engine *>(engine);
}

extern "C" void *starling_event_loop_get_engine() {
  return static_cast<void *>(api::Engine::get(api::Engine::cx()));
}

extern "C" void starling_event_loop_run_microtasks() {
  MOZ_ASSERT(EVENT_LOOP_ENGINE);
  js::RunJobs(EVENT_LOOP_ENGINE->cx());
}

extern "C" bool starling_event_loop_has_exception() {
  return JS_IsExceptionPending(EVENT_LOOP_ENGINE->cx());
}

/// Find a task by task_id, remove it from the GC-traced vector, and run it.
/// Returns 1 on success, 0 if task run failed, -1 if not found.
extern "C" int32_t starling_run_task(int32_t task_id) {
  MOZ_ASSERT(EVENT_LOOP_ENGINE);
  auto &tasks = queue.get().tasks;
  for (size_t i = 0; i < tasks.size(); i++) {
    if (tasks[i].first == task_id) {
      auto task = tasks[i].second;
      tasks.erase(tasks.begin() + i);
      return task->run(EVENT_LOOP_ENGINE) ? 1 : 0;
    }
  }
  return -1;
}
