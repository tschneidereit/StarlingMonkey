//! # starling-runtime
//!
//! The StarlingMonkey JS runtime core. This crate is the Rust port of the
//! former C++ runtime (`runtime/engine.cpp`, `runtime/script_loader.cpp`,
//! `runtime/js.cpp`, etc.).
//!
//! ## Modules
//!
//! - [`config`] — CLI argument parsing via clap, replaces `config-parser.h`
//! - [`engine`] — SpiderMonkey engine lifecycle, replaces `engine.cpp`
//! - [`script_loader`] — ES module loading and resolution, replaces `script_loader.cpp`
//! - [`strings`] — JS string ↔ UTF-8 encoding, replaces `encode.cpp`/`decode.cpp`
//! - [`allocator`] — CABI realloc for component model, replaces `allocator.cpp`
//! - [`entry`] — WASI entry points (wizer, CLI, HTTP handler), replaces `js.cpp`
//! - [`debugger`] — Optional script debugger, replaces `debugger.cpp`

pub mod allocator;
pub mod config;
#[macro_use]
pub mod rooting;
pub mod engine;
pub mod entry;
pub mod script_loader;
pub mod strings;

#[cfg(feature = "debugger")]
pub mod debugger;
