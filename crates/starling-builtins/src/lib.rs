//! C++ builtins bridge.
//!
//! This crate links the C++ builtins compiled by CMake and exposes the
//! `install_builtins` entry point to the Rust runtime.

use core::ffi::c_void;

extern "C" {
    /// Install all enabled C++ builtins into the engine.
    ///
    /// This is implemented in `builtins/install_builtins.cpp` and calls
    /// each builtin's `ns::install(engine)` function. The `engine` pointer
    /// is the `api::Engine*` from the C++ side.
    fn install_builtins(engine: *mut c_void) -> bool;
}

/// Install all enabled builtins into the engine.
///
/// # Safety
/// `engine_ptr` must be a valid `api::Engine*` from the C++ side.
pub unsafe fn install(engine_ptr: *mut c_void) -> bool {
    install_builtins(engine_ptr)
}
