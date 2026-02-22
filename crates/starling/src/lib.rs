//! StarlingMonkey — SpiderMonkey-based JS runtime for WebAssembly (WASIp3).
//!
//! This is the top-level cdylib crate that produces the final `.wasm` component.
//! It wires together:
//! - `starling-runtime` — Engine lifecycle, config, script loading
//! - `starling-host-api` — WASI p3 bindings, async event loop, HTTP handling
//! - `starling-builtins` — C++ builtins compiled via CMake
//! - `starling-sm-sys` — SpiderMonkey FFI bindings
//!
//! WASI exports (HTTP service handler, CLI run) are defined in
//! `starling-host-api/src/exports.rs` and re-exported by linking
//! that crate into this cdylib.

// Ensure all crates are linked into the final wasm binary.
// The `extern crate` declarations force the linker to include them
// even if no Rust symbols are directly referenced from this crate.
extern crate starling_runtime;
extern crate starling_host_api;
extern crate starling_builtins;
extern crate starling_sm_sys;
extern crate starling_encoding;
extern crate starling_hooks;
extern crate starling_url;
extern crate starling_multipart;
