/// WASI export implementations for WASIp3.
///
/// Uses the `wasip3` crate's export mechanisms for `wasi:http/service`
/// (replaces `wasi:http/incoming-handler` from p2) and `wasi:cli/run`.
///
/// The HTTP handler uses a trampoline model:
///   1. `starling_begin_request` — sets up the request and dispatches the fetch event
///   2. `event_loop::run()` — Rust-driven async event loop (C++ stack empty during waits)
///   3. `starling_finish_request` — post-event-loop cleanup
use wasip3::http::types::{ErrorCode, Request, Response};

extern "C" {
    /// Set up the incoming request: create HttpIncomingRequest, dispatch
    /// the JS fetch event, and run initial microtasks. Does NOT run the
    /// event loop.
    fn starling_begin_request(request_handle: i32) -> bool;

    /// Finalize request handling after the event loop completes.
    fn starling_finish_request(event_loop_success: bool) -> bool;

    /// C++ callback to run the CLI entry point.
    /// Returns true on success.
    fn starling_cli_run() -> bool;
}

/// Implementation type for the HTTP service handler.
pub struct ServiceHandler;

impl wasip3::exports::http::handler::Guest for ServiceHandler {
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        // Store the incoming request in our handle table.
        let handle = crate::http_request::insert_incoming_request(request);

        // Phase 1: Set up request and dispatch the JS fetch event.
        let ok = unsafe { starling_begin_request(handle) };
        if !ok {
            return Err(ErrorCode::InternalError(Some(
                "request initialization failed".to_string(),
            )));
        }

        // Phase 2: Run the async event loop until this request's response is ready.
        let engine = unsafe { crate::event_loop::starling_event_loop_get_engine() };
        let loop_ok = crate::event_loop::run(engine, handle).await;

        // Phase 3: Finalize (check errors, close streaming body, etc.)
        let _finish_ok = unsafe { starling_finish_request(loop_ok) };

        // Phase 4: Return the response that the JS handler stored via respondWith().
        let result = crate::http_response::take_pending_response(handle);
        result.unwrap_or(Err(ErrorCode::InternalError(Some(
            "no response was set".to_string(),
        ))))
    }
}

wasip3::http::service::export!(ServiceHandler);

/// Implementation type for the command (CLI run) world.
pub struct CommandHandler;

impl wasip3::exports::cli::run::Guest for CommandHandler {
    async fn run() -> Result<(), ()> {
        unsafe {
            if starling_cli_run() {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

wasip3::cli::command::export!(CommandHandler);
