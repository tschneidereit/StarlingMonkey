//! SpiderMonkey engine lifecycle management.
//!
//! Replaces `runtime/engine.cpp`. Owns the JSContext, globals, GC callbacks,
//! promise rejection tracking, and builtin installation.

use core::ffi::c_void;
use starling_sm_sys as sm;

use crate::config::EngineConfig;

/// Engine state, matching the C++ `api::EngineState` enum.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    Uninitialized = 0,
    EngineInitializing = 1,
    ScriptPreInitializing = 2,
    Initialized = 3,
    Aborted = 4,
}

/// The core JS runtime engine.
///
/// Owns the SpiderMonkey context, globals, and lifecycle state.
/// Only one Engine should exist at a time (SpiderMonkey limitation on wasm32).
pub struct Engine {
    config: EngineConfig,
    state: EngineState,
    cx: *mut sm::JSContext,
    /// Persistent root handle for the content global.
    content_global_handle: i32,
    /// Persistent root handle for the initializer global (-1 if none).
    init_global_handle: i32,
    /// Persistent root handle for the top-level script value (-1 if none).
    script_value_handle: i32,
}

/// Global engine pointer — set during init, used by builtins via static accessors.
static mut ENGINE: *mut Engine = core::ptr::null_mut();

impl Engine {
    /// Create and initialize a new Engine.
    ///
    /// This performs the full SM initialization sequence:
    /// 1. JS_Init, new context, self-hosted code
    /// 2. Create content + initializer globals
    /// 3. Set up promise rejection tracking
    /// 4. Install builtins
    /// 5. Eval top-level script
    pub fn new(config: EngineConfig) -> Result<Box<Engine>, &'static str> {
        // Initialize SpiderMonkey
        if !unsafe { sm::sm_init() } {
            return Err("JS_Init failed");
        }

        let cx = unsafe { sm::sm_new_context() };
        if cx.is_null() {
            return Err("Failed to create JSContext");
        }

        if !unsafe { sm::sm_init_self_hosted_code(cx) } {
            return Err("Failed to init self-hosted code");
        }

        if !unsafe { sm::sm_use_internal_job_queues(cx) } {
            return Err("Failed to init internal job queues");
        }

        // Enable Portable Baseline Interpreter
        unsafe { sm::sm_set_pbl_enabled(cx, true) };

        // Create the content global
        let content_global_handle = unsafe { sm::sm_new_global(cx) };
        if content_global_handle < 0 {
            return Err("Failed to create content global");
        }

        let mut engine = Box::new(Engine {
            config,
            state: EngineState::EngineInitializing,
            cx,
            content_global_handle,
            init_global_handle: -1,
            script_value_handle: -1,
        });

        // Store engine pointer in context private and global
        let engine_ptr = &mut *engine as *mut Engine as *mut c_void;
        unsafe {
            sm::sm_set_context_private(cx, engine_ptr);
            ENGINE = &mut *engine;
        }

        // TODO: Set up promise rejection tracking
        // TODO: Create initializer global
        // TODO: Install builtins (FFI to C++)
        // TODO: Run initializer script
        // TODO: Eval top-level content script

        engine.state = EngineState::Initialized;
        Ok(engine)
    }

    /// Get the engine singleton. Only valid after init.
    ///
    /// # Safety
    /// Must only be called after Engine::new() has completed.
    pub unsafe fn get() -> &'static mut Engine {
        debug_assert!(!ENGINE.is_null());
        &mut *ENGINE
    }

    /// Get the raw JSContext pointer.
    pub fn cx(&self) -> *mut sm::JSContext {
        self.cx
    }

    /// Get the content global object (raw pointer).
    pub fn global(&self) -> *mut sm::JSObject {
        unsafe { sm::sm_get_persistent_rooted(self.content_global_handle) }
    }

    /// Get the current engine state.
    pub fn state(&self) -> EngineState {
        self.state
    }

    /// Get the engine config.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Whether debug logging is enabled.
    pub fn debug_logging_enabled(&self) -> bool {
        self.config.verbose
    }

    /// Whether WPT mode is enabled.
    pub fn wpt_mode(&self) -> bool {
        self.config.wpt_mode
    }

    /// Set the state to Aborted.
    pub fn abort(&mut self, reason: &str) {
        eprintln!("StarlingMonkey engine aborted: {reason}");
        self.state = EngineState::Aborted;
    }

    /// Run pending microtasks.
    pub fn run_microtasks(&self) -> bool {
        unsafe { sm::sm_run_jobs(self.cx) }
    }

    /// Check if there is a pending exception.
    pub fn has_exception(&self) -> bool {
        unsafe { sm::sm_is_exception_pending(self.cx) }
    }

    /// Print the pending exception to stderr.
    pub fn dump_pending_exception(&self) {
        unsafe { sm::sm_print_pending_exception(self.cx) };
    }

    /// Transition out of pre-initialization state (after wizer snapshot).
    pub fn finish_pre_initialization(&mut self) {
        if self.state == EngineState::ScriptPreInitializing {
            self.state = EngineState::Initialized;
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if !self.cx.is_null() {
            // Drop persistent roots
            if self.content_global_handle >= 0 {
                unsafe { sm::sm_drop_persistent_rooted(self.content_global_handle) };
            }
            if self.init_global_handle >= 0 {
                unsafe { sm::sm_drop_persistent_rooted(self.init_global_handle) };
            }
            if self.script_value_handle >= 0 {
                unsafe { sm::sm_drop_persistent_rooted(self.script_value_handle) };
            }

            unsafe {
                sm::sm_destroy_context(self.cx);
                sm::sm_shutdown();
                ENGINE = core::ptr::null_mut();
            }
        }
    }
}

// ── FFI exports for C++ builtins ─────────────────────────────────────────────
//
// These are called by the rewritten extension-api.h to access engine state.

#[no_mangle]
pub unsafe extern "C" fn starling_engine_get_cx() -> *mut c_void {
    if ENGINE.is_null() {
        return core::ptr::null_mut();
    }
    (*ENGINE).cx as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn starling_engine_get_global() -> *mut c_void {
    if ENGINE.is_null() {
        return core::ptr::null_mut();
    }
    (*ENGINE).global() as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn starling_engine_get_state() -> u8 {
    if ENGINE.is_null() {
        return EngineState::Uninitialized as u8;
    }
    (*ENGINE).state as u8
}

#[no_mangle]
pub unsafe extern "C" fn starling_engine_debug_logging() -> bool {
    if ENGINE.is_null() {
        return false;
    }
    (*ENGINE).debug_logging_enabled()
}

#[no_mangle]
pub unsafe extern "C" fn starling_engine_wpt_mode() -> bool {
    if ENGINE.is_null() {
        return false;
    }
    (*ENGINE).wpt_mode()
}

#[no_mangle]
pub unsafe extern "C" fn starling_engine_abort(reason: *const u8, reason_len: u32) {
    if ENGINE.is_null() {
        return;
    }
    let reason = if reason.is_null() || reason_len == 0 {
        "unknown"
    } else {
        core::str::from_utf8(core::slice::from_raw_parts(reason, reason_len as usize))
            .unwrap_or("invalid utf8")
    };
    (*ENGINE).abort(reason);
}
