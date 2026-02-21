/**
 * WASIp3-specific request handler glue.
 *
 * In p3, the Rust async handler drives the event loop. Instead of a single
 * `starling_handle_request` that runs the full request lifecycle (including
 * the event loop), we split into:
 *   - starling_begin_request  — setup + fetch event dispatch
 *   - starling_finish_request — post-event-loop cleanup
 *
 * The Rust handler calls begin, runs its async event loop, then calls finish.
 */

#include "host_api.h"
#include "fetch_event.h"

using namespace builtins::web::fetch::fetch_event;

/// Called from Rust async handler to initialize the engine and set up the
/// incoming request. Dispatches the JS fetch event but does NOT run the
/// event loop (Rust does that asynchronously).
extern "C" bool starling_begin_request(int32_t request_handle) {
  // Ensure the engine is initialized (idempotent; no-op if wizer already set it up).
  extern bool init_from_environment();
  init_from_environment();

  // Wrap the Rust handle as a C++ HttpIncomingRequest.
  auto *request = new host_api::HttpIncomingRequest(
      std::make_unique<host_api::RustHandleState>(request_handle));

  // Dispatch the fetch event (but don't run the event loop).
  if (!begin_incoming_request(request, request_handle)) {
    return false;
  }
  return true;
}

/// Called from Rust async handler after the event loop completes.
extern "C" bool starling_finish_request(bool event_loop_success) {
  bool result = finish_incoming_request(event_loop_success);
  return result;
}
