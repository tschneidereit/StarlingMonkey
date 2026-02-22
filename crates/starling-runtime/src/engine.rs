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

#[no_mangle]
pub unsafe extern "C" fn starling_engine_debugging_enabled() -> bool {
    if ENGINE.is_null() {
        return false;
    }
    (*ENGINE).config.debugging
}

/// Get the init_location string. Returns null if not set.
/// The returned pointer is valid for the lifetime of the Engine.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_init_location(out_len: *mut u32) -> *const u8 {
    if ENGINE.is_null() || out_len.is_null() {
        return core::ptr::null();
    }
    match &(*ENGINE).config.init_location {
        Some(loc) => {
            *out_len = loc.len() as u32;
            loc.as_ptr()
        }
        None => {
            *out_len = 0;
            core::ptr::null()
        }
    }
}

/// Get the init script global (if initializer script was configured).
#[no_mangle]
pub unsafe extern "C" fn starling_engine_get_init_global() -> *mut c_void {
    if ENGINE.is_null() || (*ENGINE).init_global_handle < 0 {
        return core::ptr::null_mut();
    }
    sm::sm_get_persistent_rooted((*ENGINE).init_global_handle) as *mut c_void
}

/// Get the script value from the top-level script evaluation.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_get_script_value() -> u64 {
    if ENGINE.is_null() || (*ENGINE).script_value_handle < 0 {
        return sm::JSVAL_UNDEFINED;
    }
    let obj = sm::sm_get_persistent_rooted((*ENGINE).script_value_handle);
    if obj.is_null() {
        return sm::JSVAL_UNDEFINED;
    }
    sm::sm_object_value(obj)
}

/// Define a builtin module (called from C++ builtins via extension-api.h).
#[no_mangle]
pub unsafe extern "C" fn starling_engine_define_builtin_module(
    id: *const u8,
    id_len: u32,
    value: u64,
) -> bool {
    if ENGINE.is_null() || id.is_null() {
        return false;
    }
    let _name = match core::str::from_utf8(core::slice::from_raw_parts(id, id_len as usize)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // Delegate to the script loader's define_builtin_module
    crate::script_loader::starling_engine_define_builtin_module(
        id,
        id_len,
        value,
    )
}

/// Dump a JS value to stdout (for debugging).
#[no_mangle]
pub unsafe extern "C" fn starling_engine_dump_value(val: u64) -> bool {
    if ENGINE.is_null() {
        return false;
    }
    let cx = (*ENGINE).cx;
    let source = sm::sm_value_to_source(cx, val);
    if source.is_null() {
        return false;
    }
    let mut len: u32 = 0;
    let bytes = sm::sm_encode_string_to_utf8(cx, source, &mut len);
    if bytes.is_null() {
        return false;
    }
    let s = core::slice::from_raw_parts(bytes, len as usize);
    if let Ok(text) = core::str::from_utf8(s) {
        eprintln!("{text}");
    }
    sm::sm_free(bytes as *mut c_void);
    true
}

/// Print the current stack trace to stderr.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_print_stack() -> bool {
    if ENGINE.is_null() {
        return false;
    }
    let cx = (*ENGINE).cx;
    let mut len: u32 = 0;
    let bytes = sm::sm_capture_stack_string(cx, &mut len);
    if bytes.is_null() || len == 0 {
        return false;
    }
    let s = core::slice::from_raw_parts(bytes, len as usize);
    if let Ok(text) = core::str::from_utf8(s) {
        eprintln!("{text}");
    }
    sm::sm_free(bytes as *mut c_void);
    true
}

/// Dump a pending exception to stderr.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_dump_pending_exception(
    desc: *const u8,
    desc_len: u32,
) {
    if ENGINE.is_null() {
        return;
    }
    let cx = (*ENGINE).cx;
    if sm::sm_is_exception_pending(cx) {
        if !desc.is_null() && desc_len > 0 {
            if let Ok(text) =
                core::str::from_utf8(core::slice::from_raw_parts(desc, desc_len as usize))
            {
                eprint!("Exception while {text}: ");
            }
        }
        sm::sm_print_pending_exception(cx);
    }
}

/// Check for unhandled promise rejections.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_has_unhandled_rejections() -> bool {
    // TODO: implement with promise rejection set tracking
    false
}

/// Report unhandled promise rejections.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_report_unhandled_rejections() {
    // TODO: implement with promise rejection set tracking
}

/// Clear unhandled promise rejections.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_clear_unhandled_rejections() {
    // TODO: implement with promise rejection set tracking
}

/// Check if there are pending async tasks (delegates to event_loop.cpp's C++ side).
/// In the new architecture, this calls back into C++ because the task vector is there.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_has_pending_async_tasks() -> bool {
    extern "C" {
        fn starling_cpp_has_pending_async_tasks() -> bool;
    }
    starling_cpp_has_pending_async_tasks()
}

// ── Script evaluation FFI (called from event_loop.cpp Engine method impls) ───

/// Evaluate a top-level script from a file path.
///
/// The result value is written to `out_result` as raw JS::Value bits.
/// Returns true on success, false on error (exception will be pending on cx).
#[no_mangle]
pub unsafe extern "C" fn starling_engine_eval_toplevel_path(
    path: *const u8,
    path_len: u32,
    out_result: *mut u64,
) -> bool {
    if ENGINE.is_null() || path.is_null() || out_result.is_null() {
        return false;
    }
    let _path = match core::str::from_utf8(core::slice::from_raw_parts(path, path_len as usize)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // TODO: implement — load script from path via ScriptLoader, then eval
    // For now, delegate to the C++ scriptLoader until it's fully ported.
    *out_result = sm::JSVAL_UNDEFINED;
    false
}

/// Evaluate a top-level script from source text.
///
/// The source bytes are UTF-8 encoded JS source code. The path is used for
/// error reporting. The result value is written to `out_result` as raw
/// JS::Value bits.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_eval_toplevel_source(
    source: *const u8,
    source_len: u32,
    path: *const u8,
    path_len: u32,
    out_result: *mut u64,
) -> bool {
    if ENGINE.is_null() || source.is_null() || out_result.is_null() {
        return false;
    }
    let _source =
        match core::str::from_utf8(core::slice::from_raw_parts(source, source_len as usize)) {
            Ok(s) => s,
            Err(_) => return false,
        };
    let _path = if path.is_null() || path_len == 0 {
        "<inline>"
    } else {
        core::str::from_utf8(core::slice::from_raw_parts(path, path_len as usize))
            .unwrap_or("<invalid>")
    };
    // TODO: implement — create SourceText from source bytes, eval via ScriptLoader
    *out_result = sm::JSVAL_UNDEFINED;
    false
}

/// Run the initialization script (from the --initializer-script-path option).
#[no_mangle]
pub unsafe extern "C" fn starling_engine_run_init_script() -> bool {
    if ENGINE.is_null() {
        return false;
    }
    // TODO: implement — load and execute the initializer script in the init global
    let _init_path = match &(*ENGINE).config.initializer_script_path {
        Some(p) => p.as_str(),
        None => return true, // No init script configured — success.
    };
    false
}

/// Transition out of pre-initialization state.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_finish_pre_init() {
    if ENGINE.is_null() {
        return;
    }
    (*ENGINE).finish_pre_initialization();
}
