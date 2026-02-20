/// WASI export implementations.
///
/// Uses the `wasi` crate's proper export mechanisms (`_export_proxy!` and
/// `_export_command!` macros) so that the correct component-type metadata
/// is emitted. This ensures `wasm-component-ld` creates proper component
/// exports for `wasi:http/incoming-handler` and `wasi:cli/run`.
use wasi::http::types::{IncomingRequest, ResponseOutparam};

extern "C" {
    /// C++ callback to handle an incoming HTTP request.
    /// `request_handle` is a handle into our incoming-request table.
    /// Returns true on success.
    fn starling_handle_request(request_handle: i32) -> bool;

    /// C++ callback to run the CLI entry point.
    /// Returns true on success.
    fn starling_cli_run() -> bool;
}

/// Implementation type for the proxy (HTTP handler) world.
pub struct ProxyHandler;

impl wasi::exports::wasi::http::incoming_handler::Guest for ProxyHandler {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        // Store the response outparam globally — C++ will consume it when sending the response.
        crate::http_response::set_response_outparam(response_out);

        // Store the incoming request in our handle table.
        let handle = crate::http_request::insert_incoming_request(request);

        // Call back to C++ to process the request.
        unsafe {
            starling_handle_request(handle);
        }
    }
}

wasi::_export_proxy!(ProxyHandler);

/// Implementation type for the command (CLI run) world.
pub struct CommandHandler;

impl wasi::exports::wasi::cli::run::Guest for CommandHandler {
    fn run() -> Result<(), ()> {
        unsafe {
            if starling_cli_run() {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

wasi::_export_command!(CommandHandler);
