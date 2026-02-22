//! SpiderMonkey engine lifecycle management.
//!
//! Replaces `runtime/engine.cpp`. Owns the JSContext, globals, GC callbacks,
//! promise rejection tracking, and builtin installation.

use core::ffi::c_void;
use starling_sm_sys as sm;

use crate::config::EngineConfig;
use crate::script_loader::ScriptLoader;

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
    /// Persistent root handle for the unhandled rejection promise set.
    unhandled_rejections_handle: i32,
    /// The script/module loader.
    script_loader: Option<ScriptLoader>,
}

/// Global engine pointer — set during init, used by builtins via static accessors.
static mut ENGINE: *mut Engine = core::ptr::null_mut();

// ── C++ FFI declarations ─────────────────────────────────────────────────────
//
// Functions provided by the C++ side (event_loop.cpp, install_builtins.cpp, etc.)

extern "C" {
    fn starling_cpp_has_pending_async_tasks() -> bool;
    fn starling_find_and_run_immediate_task() -> i32;
    fn starling_event_loop_set_engine(engine: *mut c_void);
    fn install_builtins(engine: *mut c_void) -> bool;
}

impl Engine {
    /// Create and initialize a new Engine.
    ///
    /// This performs the full SM initialization sequence:
    /// 1. JS_Init, new context, self-hosted code
    /// 2. Create content + initializer globals
    /// 3. Set up promise rejection tracking
    /// 4. Create script loader
    /// 5. Install builtins (FFI to C++)
    /// 6. Run initializer script (if configured)
    /// 7. Eval top-level script
    pub fn new(config: EngineConfig) -> Result<Box<Engine>, &'static str> {
        // Initialize SpiderMonkey
        if !unsafe { sm::sm_init() } {
            return Err("JS_Init failed");
        }

        let cx = unsafe { sm::sm_new_context() };
        if cx.is_null() {
            return Err("Failed to create JSContext");
        }

        // UseInternalJobQueues must be called BEFORE InitSelfHostedCode.
        // Self-hosted code may use promises, which require the job queue.
        if !unsafe { sm::sm_use_internal_job_queues(cx) } {
            return Err("Failed to init internal job queues");
        }

        if !unsafe { sm::sm_init_self_hosted_code(cx) } {
            return Err("Failed to init self-hosted code");
        }

        // Enable Portable Baseline Interpreter if configured via env
        if std::env::var("ENABLE_PBL").as_deref() == Ok("1") {
            unsafe { sm::sm_set_pbl_enabled(cx, true) };
        }

        // Create the content global
        let content_global_handle = unsafe { sm::sm_new_global(cx) };
        if content_global_handle < 0 {
            return Err("Failed to create content global");
        }

        // Enter the content global's realm — must be in-realm before any
        // allocations (fix_math_random, NewSetObject) or SM's GC will trap.
        let content_global = unsafe { sm::sm_get_persistent_rooted(content_global_handle) };
        let _old_realm = unsafe { sm::sm_enter_realm(cx, content_global) };

        // Fix Math.random to use WASI random instead of SM's PRNG
        if !unsafe { sm::sm_fix_math_random(cx, content_global, Some(math_random_wasi)) } {
            return Err("Failed to fix Math.random");
        }

        // Create the unhandled rejections set
        let unhandled_rejections_handle = unsafe { sm::sm_new_set(cx) };
        if unhandled_rejections_handle < 0 {
            return Err("Failed to create unhandled rejections set");
        }

        let mut engine = Box::new(Engine {
            config,
            state: EngineState::EngineInitializing,
            cx,
            content_global_handle,
            init_global_handle: -1,
            script_value_handle: -1,
            unhandled_rejections_handle,
            script_loader: None,
        });

        // Store engine pointer in context private and global
        let engine_ptr = &mut *engine as *mut Engine as *mut c_void;
        unsafe {
            sm::sm_set_context_private(cx, engine_ptr);
            ENGINE = &mut *engine;
        }

        // Set up promise rejection tracking
        unsafe {
            sm::sm_set_promise_rejection_tracker(cx, Some(rejection_tracker), core::ptr::null_mut());
        }

        // Create the script loader
        let mut script_loader = ScriptLoader::new(cx)?;
        script_loader.set_path_prefix(engine.config.path_prefix.clone());
        engine.script_loader = Some(script_loader);

        // Create the initializer global (same compartment as content)
        let init_global_handle = unsafe {
            sm::sm_new_global_same_compartment(cx, content_global)
        };
        if init_global_handle < 0 {
            return Err("Failed to create initializer global");
        }
        engine.init_global_handle = init_global_handle;

        // Define helper functions on the init global
        let init_global = unsafe { sm::sm_get_persistent_rooted(init_global_handle) };
        setup_init_global(cx, init_global, content_global)?;

        // Initialize the event loop (C++ side)
        extern "C" {
            fn starling_event_loop_init(cx: *mut c_void);
        }
        unsafe { starling_event_loop_init(cx) };

        // Install C++ builtins
        let engine_for_builtins = &mut *engine as *mut Engine as *mut c_void;
        if !unsafe { install_builtins(engine_for_builtins) } {
            return Err("Failed to install builtins");
        }

        // Run initialization script if configured
        if engine.config.initializer_script_path.is_some() {
            if !engine.run_initialization_script() {
                return Err("Failed to run initialization script");
            }
        }

        // Set state based on config
        if engine.config.pre_initialize {
            engine.state = EngineState::ScriptPreInitializing;
        } else {
            engine.state = EngineState::Initialized;
        }

        // Enable module mode on the script loader
        if let Some(ref mut loader) = engine.script_loader {
            loader.set_module_mode(engine.config.module_mode());
        }

        // Eval content script if configured
        if let Some(script) = engine.config.content_script().map(|s| s.to_string()) {
            // Inline eval script
            let path = "<eval>";
            if !engine.eval_toplevel_source(script.as_bytes(), path) {
                return Err("Failed to evaluate inline script");
            }
        } else {
            let path = engine.config.script_path.clone();
            if !engine.eval_toplevel_path(&path) {
                return Err("Failed to evaluate top-level script");
            }
        }

        Ok(engine)
    }

    /// Check if the engine has been initialized (e.g., by wizer).
    pub fn is_initialized() -> bool {
        unsafe { !ENGINE.is_null() }
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

    /// Get the script loader, if initialized.
    pub fn script_loader(&self) -> Option<&ScriptLoader> {
        self.script_loader.as_ref()
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

    /// Check for unhandled promise rejections.
    pub fn has_unhandled_rejections(&self) -> bool {
        if self.unhandled_rejections_handle < 0 {
            return false;
        }
        unsafe { sm::sm_set_size(self.cx, self.unhandled_rejections_handle) > 0 }
    }

    /// Report unhandled promise rejections to stderr.
    pub fn report_unhandled_rejections(&self) {
        if !self.has_unhandled_rejections() {
            return;
        }
        let cx = self.cx;
        unsafe {
            sm::sm_set_for_each(
                cx,
                self.unhandled_rejections_handle,
                Some(report_rejection_callback),
                cx as *mut c_void,
            );
        }
    }

    /// Clear unhandled promise rejections.
    pub fn clear_unhandled_rejections(&self) {
        if self.unhandled_rejections_handle >= 0 {
            unsafe { sm::sm_set_clear(self.cx, self.unhandled_rejections_handle) };
        }
    }

    /// Transition out of pre-initialization state (after wizer snapshot).
    pub fn finish_pre_initialization(&mut self) {
        if self.state == EngineState::ScriptPreInitializing {
            unsafe { sm::sm_reset_math_random_seed(self.cx) };
            self.state = EngineState::Initialized;
        }
    }

    /// Run the synchronous event loop (for pre-initialization / wizer).
    ///
    /// Runs microtasks, then finds and runs immediate tasks until no more
    /// remain. This is the Rust-owned sync loop, replacing the C++ version.
    /// Only immediate tasks (IMMEDIATE_TASK_HANDLE) are expected during sync
    /// execution (e.g., wizer pre-init).
    fn run_sync_event_loop(&self) -> bool {
        let cx = self.cx;
        let engine_ptr = self as *const Engine as *mut c_void;

        // Set the C++ EVENT_LOOP_ENGINE so starling_find_and_run_immediate_task
        // can pass it to task->run(engine).
        unsafe { starling_event_loop_set_engine(engine_ptr) };

        loop {
            // Run microtasks
            unsafe { sm::sm_run_jobs(cx) };

            // Check for exceptions from microtasks
            if unsafe { sm::sm_is_exception_pending(cx) } {
                return false;
            }

            // Check if there are any pending tasks
            if !unsafe { starling_cpp_has_pending_async_tasks() } {
                return true;
            }

            // Find and run the next immediate task
            let result = unsafe { starling_find_and_run_immediate_task() };
            match result {
                1 => continue,  // Task ran successfully, loop again
                0 => return false, // Task run failed
                _ => return false, // No immediate task found (shouldn't happen during sync)
            }
        }
    }

    /// Run the initialization script in the init global's realm.
    fn run_initialization_script(&self) -> bool {
        let init_path = match &self.config.initializer_script_path {
            Some(p) => p.clone(),
            None => return true, // No init script configured
        };

        let cx = self.cx;
        let init_global = unsafe { sm::sm_get_persistent_rooted(self.init_global_handle) };
        if init_global.is_null() {
            return false;
        }

        // Switch to the init global's realm
        let old_realm = unsafe { sm::sm_enter_realm(cx, init_global) };

        // Read the init script file
        let source = match std::fs::read(&init_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("Failed to read initialization script '{init_path}': {e}");
                unsafe { sm::sm_leave_realm(cx, old_realm) };
                return false;
            }
        };

        // Compile and execute as a classic script (not a module)
        let mut result: sm::JSVal = sm::JSVAL_UNDEFINED;
        let ok = unsafe {
            sm::sm_evaluate_script(
                cx,
                init_global,
                source.as_ptr(),
                source.len() as u32,
                init_path.as_ptr(),
                init_path.len() as u32,
                &mut result,
            )
        };

        // Restore the content global's realm
        unsafe { sm::sm_leave_realm(cx, old_realm) };

        ok
    }

    /// Evaluate a top-level script from a file path.
    fn eval_toplevel_path(&mut self, path: &str) -> bool {
        // Read the file
        let source = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("Failed to read script '{path}': {e}");
                return false;
            }
        };

        self.eval_toplevel_source(&source, path)
    }

    /// Evaluate a top-level script from source bytes.
    fn eval_toplevel_source(&mut self, source: &[u8], path: &str) -> bool {
        let cx = self.cx;
        let module_mode = self.config.module_mode();
        let content_global = self.global();

        // Use as_mut() so the loader stays in self — module resolve hooks
        // call Engine::get().script_loader() and need to find Some(_).
        let loader = match self.script_loader.as_mut() {
            Some(l) => l,
            None => return false,
        };

        let result = loader.eval_top_level_script(source, path, module_mode, content_global);

        let script_val = match result {
            Ok(val) => val,
            Err(_e) => {
                eprintln!("Exception while evaluating top-level script");
                if unsafe { sm::sm_is_exception_pending(cx) } {
                    unsafe { sm::sm_print_pending_exception(cx) };
                }
                return false;
            }
        };

        // Root the script value (typically the module eval promise) across
        // the sync event loop, which can trigger GC.
        rooted_value!(in(cx) let script_val_rooted = script_val);

        // Store the script value as a persistent root
        // For modules, this is the namespace object. For scripts, typically undefined.
        if unsafe { sm::sm_value_is_object(script_val) } {
            let obj = unsafe { sm::sm_value_to_object(script_val) };
            if !obj.is_null() {
                // If there's a previous script value, drop it
                if self.script_value_handle >= 0 {
                    unsafe { sm::sm_drop_persistent_rooted(self.script_value_handle) };
                }
                // The module namespace is returned from eval_as_module.
                // For the script case, we get the eval result.
                // Store the module namespace as the script value.
                if module_mode {
                    // Get the module namespace from the loader's last evaluated module
                    // The script_val from eval_as_module is the eval promise,
                    // but we need the namespace. Let's store the object.
                    // TODO: Store module namespace properly. For now, the
                    // SCRIPT_VALUE will be set by the C++ side if needed.
                }
            }
        }

        // Run the synchronous event loop (for pre-init / immediate tasks)
        self.run_sync_event_loop();

        // Check for TLA (top-level await) rejection — use the rooted value
        // since GC may have run during the sync event loop.
        if module_mode && unsafe { sm::sm_value_is_object(script_val_rooted.get()) } {
            let promise_obj = unsafe { sm::sm_value_to_object(script_val_rooted.get()) };
            if !promise_obj.is_null() {
                rooted_object!(in(cx) let promise_rooted = promise_obj);
                let state = unsafe { sm::sm_get_promise_state(cx, promise_rooted.get()) };
                if state == sm::PROMISE_STATE_REJECTED {
                    let result_bits = unsafe { sm::sm_get_promise_result(cx, promise_rooted.get()) };
                    unsafe { sm::sm_set_pending_exception(cx, result_bits) };
                    eprintln!("Exception while evaluating top-level script");
                    unsafe { sm::sm_print_pending_exception(cx) };
                    return false;
                }
            }
        }

        // Report any unhandled promise rejections
        if self.has_unhandled_rejections() {
            self.report_unhandled_rejections();
        }

        // Run GC during pre-initialization
        if self.state == EngineState::ScriptPreInitializing {
            unsafe { sm::sm_gc(cx) };
        }

        true
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if !self.cx.is_null() {
            // Drop script loader first (it has persistent roots)
            self.script_loader.take();

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
            // Note: don't drop unhandled_rejections_handle here —
            // it's a Set in the content global's realm and will be GC'd.

            unsafe {
                sm::sm_destroy_context(self.cx);
                sm::sm_shutdown();
                ENGINE = core::ptr::null_mut();
            }
        }
    }
}

// ── Helper functions ─────────────────────────────────────────────────────────

/// Math.random replacement using WASI random.
unsafe extern "C" fn math_random_wasi(
    _cx: *mut sm::JSContext,
    argc: u32,
    vp: *mut sm::JSVal,
) -> bool {
    // Generate a random u32 via WASI. On wasm32-wasip2, getrandom is available.
    let mut buf = [0u8; 4];
    getrandom(&mut buf);
    let random_u32 = u32::from_le_bytes(buf);
    let value = (random_u32 as f64) / (2.0_f64.powi(32));
    sm::sm_call_args_rval_set(argc, vp, sm::sm_double_value(value));
    true
}

/// Fill a buffer with random bytes using WASI random.
fn getrandom(buf: &mut [u8]) {
    // Use the standard getrandom approach available on wasm32-wasip2.
    // The Rust standard library routes this through wasi:random/random.
    #[cfg(target_os = "wasi")]
    {
        // On WASI, std::io has no direct getrandom, but we can use
        // the libc-level random_get.
        extern "C" {
            fn __wasi_random_get(buf: *mut u8, buf_len: usize) -> u16;
        }
        unsafe {
            __wasi_random_get(buf.as_mut_ptr(), buf.len());
        }
    }
    #[cfg(not(target_os = "wasi"))]
    {
        // Fallback for non-WASI builds (testing, etc.)
        for b in buf.iter_mut() {
            *b = 42; // deterministic fallback
        }
    }
}

/// Promise rejection tracker callback.
///
/// Called by SpiderMonkey when a promise is rejected without a handler,
/// or when a previously-unhandled rejection gets a handler attached.
unsafe extern "C" fn rejection_tracker(
    _cx: *mut sm::JSContext,
    promise: *mut sm::JSObject,
    state: u32,
    _data: *mut c_void,
) {
    if ENGINE.is_null() {
        return;
    }
    let engine = &*ENGINE;
    let set_handle = engine.unhandled_rejections_handle;
    if set_handle < 0 {
        return;
    }
    let cx = engine.cx;
    let promise_val = sm::sm_object_value(promise);

    // state 0 = Unhandled, 1 = Handled (matching JS::PromiseRejectionHandlingState)
    if state == 0 {
        // Unhandled rejection — add to the set
        sm::sm_set_add(cx, set_handle, promise_val);
    } else {
        // Handled — remove from the set
        sm::sm_set_delete(cx, set_handle, promise_val);
    }
}

/// Callback for reporting each unhandled rejection during iteration.
unsafe extern "C" fn report_rejection_callback(val: sm::JSVal, data: *mut c_void) -> bool {
    let cx = data as *mut sm::JSContext;
    if sm::sm_value_is_object(val) {
        let promise = sm::sm_value_to_object(val);
        let state = sm::sm_get_promise_state(cx, promise);
        if state == sm::PROMISE_STATE_REJECTED {
            let result_bits = sm::sm_get_promise_result(cx, promise);
            eprint!("Promise rejected but never handled: ");
            // Try to dump the rejection value
            let source = sm::sm_value_to_source(cx, result_bits);
            if !source.is_null() {
                let mut len: u32 = 0;
                let bytes = sm::sm_encode_string_to_utf8(cx, source, &mut len);
                if !bytes.is_null() {
                    let s = core::slice::from_raw_parts(bytes, len as usize);
                    if let Ok(text) = core::str::from_utf8(s) {
                        eprintln!("{text}");
                    }
                    sm::sm_free(bytes as *mut c_void);
                } else {
                    eprintln!("<unable to encode>");
                }
            } else {
                eprintln!("<unable to stringify>");
            }
        }
    }
    true // continue iteration
}

/// Set up helper functions on the init global (defineBuiltinModule, contentGlobal).
fn setup_init_global(
    cx: *mut sm::JSContext,
    init_global: *mut sm::JSObject,
    content_global: *mut sm::JSObject,
) -> Result<(), &'static str> {
    // Switch to init global realm temporarily
    let old_realm = unsafe { sm::sm_enter_realm(cx, init_global) };

    // defineBuiltinModule(name, value)
    let name = b"defineBuiltinModule\0";
    if !unsafe {
        sm::sm_define_function(
            cx,
            init_global,
            name.as_ptr(),
            name.len() as u32 - 1, // exclude null terminator
            Some(init_define_builtin_module),
            2,
            0,
        )
    } {
        unsafe { sm::sm_leave_realm(cx, old_realm) };
        return Err("Failed to define defineBuiltinModule on init global");
    }

    // contentGlobal property
    let content_global_val = unsafe { sm::sm_object_value(content_global) };
    let prop_name = b"contentGlobal\0";
    // JSPROP_READONLY = 0x10 in SM
    if !unsafe {
        sm::sm_define_property_value(
            cx,
            init_global,
            prop_name.as_ptr(),
            prop_name.len() as u32 - 1,
            content_global_val,
            0x10, // JSPROP_READONLY
        )
    } {
        unsafe { sm::sm_leave_realm(cx, old_realm) };
        return Err("Failed to define contentGlobal on init global");
    }

    unsafe { sm::sm_leave_realm(cx, old_realm) };
    Ok(())
}

/// JS native: defineBuiltinModule(name, value)
/// Called from initialization scripts to register builtin modules.
unsafe extern "C" fn init_define_builtin_module(
    cx: *mut sm::JSContext,
    argc: u32,
    vp: *mut sm::JSVal,
) -> bool {
    let name_val = sm::sm_call_args_get(argc, vp, 0);
    let value_val = sm::sm_call_args_get(argc, vp, 1);

    if !sm::sm_value_is_string(name_val) {
        sm::sm_report_error(
            cx,
            b"First argument to defineBuiltinModule must be a string".as_ptr(),
            51,
        );
        return false;
    }
    if !sm::sm_value_is_object(value_val) {
        sm::sm_report_error(
            cx,
            b"Second argument to defineBuiltinModule must be an object".as_ptr(),
            53,
        );
        return false;
    }

    let name_str = sm::sm_value_to_string(name_val);
    let mut name_len: u32 = 0;
    let name_bytes = sm::sm_encode_string_to_utf8(cx, name_str, &mut name_len);
    if name_bytes.is_null() {
        return false;
    }

    let result = starling_engine_define_builtin_module(name_bytes, name_len, value_val);
    sm::sm_free(name_bytes as *mut c_void);

    if result {
        sm::sm_call_args_rval_set_undefined(argc, vp);
    }
    result
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
    let name = match core::str::from_utf8(core::slice::from_raw_parts(id, id_len as usize)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    match &(*ENGINE).script_loader {
        Some(loader) => loader.define_builtin_module(name, value),
        None => false,
    }
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
    if ENGINE.is_null() {
        return false;
    }
    (*ENGINE).has_unhandled_rejections()
}

/// Report unhandled promise rejections.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_report_unhandled_rejections() {
    if ENGINE.is_null() {
        return;
    }
    (*ENGINE).report_unhandled_rejections();
}

/// Clear unhandled promise rejections.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_clear_unhandled_rejections() {
    if ENGINE.is_null() {
        return;
    }
    (*ENGINE).clear_unhandled_rejections();
}

/// Check if there are pending async tasks (delegates to event_loop.cpp's C++ side).
#[no_mangle]
pub unsafe extern "C" fn starling_engine_has_pending_async_tasks() -> bool {
    starling_cpp_has_pending_async_tasks()
}

// ── Script evaluation FFI (called from event_loop.cpp Engine method impls) ───

/// Evaluate a top-level script from a file path.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_eval_toplevel_path(
    path: *const u8,
    path_len: u32,
    out_result: *mut u64,
) -> bool {
    if ENGINE.is_null() || path.is_null() || out_result.is_null() {
        return false;
    }
    let path_str = match core::str::from_utf8(core::slice::from_raw_parts(path, path_len as usize))
    {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Read the file
    let source = match std::fs::read(path_str) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Failed to read script '{path_str}': {e}");
            return false;
        }
    };

    // Evaluate via the script loader
    let engine = &mut *ENGINE;
    let module_mode = engine.config.module_mode();
    let content_global = engine.global();

    let mut loader = match engine.script_loader.take() {
        Some(l) => l,
        None => return false,
    };

    let result = loader.eval_top_level_script(&source, path_str, module_mode, content_global);
    engine.script_loader = Some(loader);

    match result {
        Ok(val) => {
            *out_result = val;
            true
        }
        Err(e) => {
            eprintln!("Failed to evaluate script: {e}");
            false
        }
    }
}

/// Evaluate a top-level script from source text.
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
    let source_bytes = core::slice::from_raw_parts(source, source_len as usize);
    let path_str = if path.is_null() || path_len == 0 {
        "<inline>"
    } else {
        core::str::from_utf8(core::slice::from_raw_parts(path, path_len as usize))
            .unwrap_or("<invalid>")
    };

    let engine = &mut *ENGINE;
    let module_mode = engine.config.module_mode();
    let content_global = engine.global();

    let mut loader = match engine.script_loader.take() {
        Some(l) => l,
        None => return false,
    };

    let result = loader.eval_top_level_script(source_bytes, path_str, module_mode, content_global);
    engine.script_loader = Some(loader);

    match result {
        Ok(val) => {
            *out_result = val;
            true
        }
        Err(e) => {
            eprintln!("Failed to evaluate script: {e}");
            false
        }
    }
}

/// Run the initialization script (from the --initializer-script-path option).
#[no_mangle]
pub unsafe extern "C" fn starling_engine_run_init_script() -> bool {
    if ENGINE.is_null() {
        return false;
    }
    (*ENGINE).run_initialization_script()
}

/// Transition out of pre-initialization state.
#[no_mangle]
pub unsafe extern "C" fn starling_engine_finish_pre_init() {
    if ENGINE.is_null() {
        return;
    }
    (*ENGINE).finish_pre_initialization();
}
