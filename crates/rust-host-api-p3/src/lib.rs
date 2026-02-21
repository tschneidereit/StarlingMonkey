mod cli;
mod clocks;
mod event_loop;
mod exports;
mod handle_table;
mod http_body;
mod http_headers;
mod http_request;
mod http_response;
mod poll;
mod random;
mod sockets;
mod task_queue;

// We use `-nostartfiles` to omit crt1-reactor.o and provide our own
// `_initialize` that calls `run_ctors_once()` instead of `__wasm_call_ctors`
// directly. This ensures the wit-bindgen `RUN` flag is set to `true` in the
// wizer snapshot, preventing double-initialization of C++ statics when the
// first export is called after wizer resume.
#[unsafe(no_mangle)]
pub extern "C" fn _initialize() {
    wit_bindgen::rt::run_ctors_once();
}
