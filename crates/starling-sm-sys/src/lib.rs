//! # starling-sm-sys
//!
//! Low-level FFI bindings to SpiderMonkey via a C++ shim layer.
//!
//! This crate wraps SpiderMonkey's C++ API as `extern "C"` functions that Rust
//! can call directly. The C++ shim (in `shim/`) handles all SM-specific concerns:
//! rooting, GC integration, realm management, etc.
//!
//! # Safety
//!
//! All functions in this crate are unsafe. The caller must ensure:
//! - `sm_init()` is called exactly once before any other function.
//! - `sm_new_context()` is called before any context-dependent function.
//! - GC rooting constraints are respected (values must be rooted before use).

#![no_std]

use core::ffi::c_void;

// ── Opaque SM types ──────────────────────────────────────────────────────────

/// Opaque SpiderMonkey JSContext.
pub type JSContext = c_void;

/// Opaque SpiderMonkey JSObject.
pub type JSObject = c_void;

/// Opaque SpiderMonkey JSString.
pub type JSString = c_void;

/// Opaque SpiderMonkey JSFunction.
pub type JSFunction = c_void;

/// Opaque SpiderMonkey JSScript.
pub type JSScript = c_void;

/// Opaque GC tracer.
pub type JSTracer = c_void;

/// SpiderMonkey JS::Value — 8-byte NaN-boxed value.
///
/// On wasm32. SM uses a specific NaN-boxing scheme. We treat this
/// as an opaque 64-bit value and provide helpers to construct/inspect.
pub type JSVal = u64;

// ── Constants ────────────────────────────────────────────────────────────────

/// JS::UndefinedValue() NaN-boxed representation (wasm32 nunboxing).
pub const JSVAL_UNDEFINED: JSVal = 0xFFFF_FFF6_8000_0000;
/// JS::NullValue() NaN-boxed representation.
pub const JSVAL_NULL: JSVal = 0xFFFF_FFF5_8000_0000;
/// JS::TrueValue().
pub const JSVAL_TRUE: JSVal = 0xFFFF_FFF3_0000_0001;
/// JS::FalseValue().
pub const JSVAL_FALSE: JSVal = 0xFFFF_FFF3_0000_0000;

// ── Initialization & context ─────────────────────────────────────────────────

extern "C" {
    /// Initialize the SpiderMonkey runtime. Must be called once before any other SM function.
    pub fn sm_init() -> bool;

    /// Shut down the SpiderMonkey runtime. Call after destroying all contexts.
    pub fn sm_shutdown();

    /// Create a new JSContext. Returns null on failure.
    pub fn sm_new_context() -> *mut JSContext;

    /// Destroy a JSContext.
    pub fn sm_destroy_context(cx: *mut JSContext);

    /// Initialize self-hosted code (required after context creation).
    pub fn sm_init_self_hosted_code(cx: *mut JSContext) -> bool;

    /// Enable internal job queues (microtask queue) for the context.
    pub fn sm_use_internal_job_queues(cx: *mut JSContext) -> bool;

    /// Set the Portable Baseline Interpreter option.
    pub fn sm_set_pbl_enabled(cx: *mut JSContext, enabled: bool);

    /// Store a private pointer in the context (for Engine retrieval).
    pub fn sm_set_context_private(cx: *mut JSContext, data: *mut c_void);

    /// Retrieve the private pointer from the context.
    pub fn sm_get_context_private(cx: *mut JSContext) -> *mut c_void;
}

// ── Globals & realms ─────────────────────────────────────────────────────────

extern "C" {
    /// Create a new global object. Returns a persistent root handle (index).
    /// The global is initialized with standard classes.
    pub fn sm_new_global(cx: *mut JSContext) -> i32;

    /// Get the raw JSObject* for a persistent root handle.
    pub fn sm_get_persistent_rooted(handle: i32) -> *mut JSObject;

    /// Drop a persistent root handle, allowing GC to collect it.
    pub fn sm_drop_persistent_rooted(handle: i32);

    /// Enter the realm of the given global object.
    pub fn sm_enter_realm(cx: *mut JSContext, global: *mut JSObject) -> *mut c_void;

    /// Leave the current realm, restoring the previous one.
    pub fn sm_leave_realm(cx: *mut JSContext, old_realm: *mut c_void);

    /// Define a function on an object.
    pub fn sm_define_function(
        cx: *mut JSContext,
        obj: *mut JSObject,
        name: *const u8,
        name_len: u32,
        func: Option<unsafe extern "C" fn(*mut JSContext, u32, *mut JSVal) -> bool>,
        nargs: u32,
        flags: u32,
    ) -> bool;

    /// Define a property with a value.
    pub fn sm_define_property_value(
        cx: *mut JSContext,
        obj: *mut JSObject,
        name: *const u8,
        name_len: u32,
        value: JSVal,
        flags: u32,
    ) -> bool;
}

// ── Script compilation & evaluation ──────────────────────────────────────────

extern "C" {
    /// Compile and evaluate a script (non-module).
    /// Returns the result value. On failure, returns JSVAL_UNDEFINED and sets
    /// a pending exception on the context.
    pub fn sm_evaluate_script(
        cx: *mut JSContext,
        global: *mut JSObject,
        source: *const u8,
        source_len: u32,
        filename: *const u8,
        filename_len: u32,
        result: *mut JSVal,
    ) -> bool;

    /// Compile an ES module from source text.
    /// Returns a persistent root handle to the module object, or -1 on failure.
    pub fn sm_compile_module(
        cx: *mut JSContext,
        source: *const u8,
        source_len: u32,
        filename: *const u8,
        filename_len: u32,
    ) -> i32;

    /// Link a compiled module (resolve imports).
    pub fn sm_module_link(cx: *mut JSContext, module_handle: i32) -> bool;

    /// Evaluate a linked module. Returns the evaluation promise handle, or -1.
    pub fn sm_module_evaluate(cx: *mut JSContext, module_handle: i32) -> i32;

    /// Set the module resolve hook. The callback receives (cx, referencing_private, specifier)
    /// and must return a module object handle or -1.
    pub fn sm_set_module_resolve_hook(
        cx: *mut JSContext,
        hook: Option<
            unsafe extern "C" fn(
                cx: *mut JSContext,
                referencing_private: JSVal,
                specifier: *mut JSString,
            ) -> i32,
        >,
    );

    /// Set the module metadata hook. The callback receives (cx, module_private, meta_object).
    pub fn sm_set_module_metadata_hook(
        cx: *mut JSContext,
        hook: Option<
            unsafe extern "C" fn(
                cx: *mut JSContext,
                module_private: JSVal,
                meta_object: *mut JSObject,
            ) -> bool,
        >,
    );

    /// Get the module request specifier as a JSString.
    pub fn sm_get_module_request_specifier(
        cx: *mut JSContext,
        module_request: *mut JSObject,
    ) -> *mut JSString;

    /// Set the private value on a module object (used for the module path).
    pub fn sm_set_module_private(module: *mut JSObject, private_value: JSVal);

    /// Get the module namespace object.
    pub fn sm_get_module_namespace(cx: *mut JSContext, module_handle: i32) -> *mut JSObject;
}

// ── String operations ────────────────────────────────────────────────────────

extern "C" {
    /// Encode a JSString to UTF-8. The caller must free the returned buffer with sm_free().
    /// Returns the length via out_len. Returns null on failure.
    pub fn sm_encode_string_to_utf8(
        cx: *mut JSContext,
        str: *mut JSString,
        out_len: *mut u32,
    ) -> *mut u8;

    /// Create a JSString from UTF-8 bytes.
    pub fn sm_new_string_utf8(
        cx: *mut JSContext,
        bytes: *const u8,
        len: u32,
    ) -> *mut JSString;

    /// Create a JSString from Latin1 bytes.
    pub fn sm_new_string_latin1(
        cx: *mut JSContext,
        bytes: *const u8,
        len: u32,
    ) -> *mut JSString;

    /// Get the length of a JSString in UTF-16 code units.
    pub fn sm_get_string_length(str: *mut JSString) -> u32;

    /// Free a buffer allocated by the SM shim (e.g., from encode_string_to_utf8).
    pub fn sm_free(ptr: *mut c_void);
}

// ── Value operations ─────────────────────────────────────────────────────────

extern "C" {
    /// Box an i32 into a JSVal.
    pub fn sm_int32_value(v: i32) -> JSVal;

    /// Box a f64 into a JSVal.
    pub fn sm_double_value(v: f64) -> JSVal;

    /// Box a JSString* into a JSVal.
    pub fn sm_string_value(str: *mut JSString) -> JSVal;

    /// Box a JSObject* into a JSVal.
    pub fn sm_object_value(obj: *mut JSObject) -> JSVal;

    /// Box a bool into a JSVal.
    pub fn sm_boolean_value(v: bool) -> JSVal;

    /// Extract an object pointer from a JSVal. Returns null if not an object.
    pub fn sm_value_to_object(val: JSVal) -> *mut JSObject;

    /// Check if a JSVal is an object.
    pub fn sm_value_is_object(val: JSVal) -> bool;

    /// Check if a JSVal is undefined.
    pub fn sm_value_is_undefined(val: JSVal) -> bool;

    /// Check if a JSVal is null.
    pub fn sm_value_is_null(val: JSVal) -> bool;

    /// Check if a JSVal is a string.
    pub fn sm_value_is_string(val: JSVal) -> bool;

    /// Extract a string from a JSVal. Returns null if not a string.
    pub fn sm_value_to_string(val: JSVal) -> *mut JSString;

    /// Get a property from an object by name.
    pub fn sm_get_property(
        cx: *mut JSContext,
        obj: *mut JSObject,
        name: *const u8,
        name_len: u32,
        result: *mut JSVal,
    ) -> bool;

    /// Set a property on an object by name.
    pub fn sm_set_property(
        cx: *mut JSContext,
        obj: *mut JSObject,
        name: *const u8,
        name_len: u32,
        value: JSVal,
    ) -> bool;

    /// Call a function value with arguments.
    pub fn sm_call_function(
        cx: *mut JSContext,
        this_obj: *mut JSObject,
        func: JSVal,
        argc: u32,
        argv: *const JSVal,
        result: *mut JSVal,
    ) -> bool;
}

// ── Promise operations ───────────────────────────────────────────────────────

/// Promise states matching JS::PromiseState.
pub const PROMISE_STATE_PENDING: u32 = 0;
pub const PROMISE_STATE_FULFILLED: u32 = 1;
pub const PROMISE_STATE_REJECTED: u32 = 2;

extern "C" {
    /// Set the promise rejection tracker callback.
    /// The callback signature: (cx, promise_obj, state_u32, data_ptr).
    pub fn sm_set_promise_rejection_tracker(
        cx: *mut JSContext,
        hook: Option<
            unsafe extern "C" fn(
                cx: *mut JSContext,
                promise: *mut JSObject,
                state: u32,
                data: *mut c_void,
            ),
        >,
        data: *mut c_void,
    );

    /// Get the state of a promise (pending/fulfilled/rejected).
    pub fn sm_get_promise_state(cx: *mut JSContext, promise: *mut JSObject) -> u32;

    /// Get the result value of a settled promise (as raw bits).
    pub fn sm_get_promise_result(cx: *mut JSContext, promise: *mut JSObject) -> u64;

    /// Create a new promise object.
    pub fn sm_new_promise(cx: *mut JSContext) -> *mut JSObject;

    /// Resolve a promise with a value.
    pub fn sm_resolve_promise(cx: *mut JSContext, promise: *mut JSObject, value: JSVal) -> bool;

    /// Reject a promise with a reason.
    pub fn sm_reject_promise(cx: *mut JSContext, promise: *mut JSObject, reason: JSVal) -> bool;
}

// ── GC operations ────────────────────────────────────────────────────────────

extern "C" {
    /// Run a full, non-incremental GC.
    pub fn sm_gc(cx: *mut JSContext);

    /// Run pending microtasks (job queue).
    pub fn sm_run_jobs(cx: *mut JSContext) -> bool;

    /// Add an extra GC roots tracer callback.
    pub fn sm_add_extra_gc_roots_tracer(
        cx: *mut JSContext,
        tracer: Option<unsafe extern "C" fn(trc: *mut JSTracer, data: *mut c_void)>,
        data: *mut c_void,
    );
}

// ── Errors ───────────────────────────────────────────────────────────────────

extern "C" {
    /// Check if there is a pending exception on the context.
    pub fn sm_is_exception_pending(cx: *mut JSContext) -> bool;

    /// Clear the pending exception.
    pub fn sm_clear_pending_exception(cx: *mut JSContext);

    /// Steal the pending exception (value + stack). Returns the exception value.
    pub fn sm_steal_pending_exception(cx: *mut JSContext, out_val: *mut JSVal) -> bool;

    /// Report a UTF-8 error string on the context (sets pending exception).
    pub fn sm_report_error(cx: *mut JSContext, msg: *const u8, msg_len: u32);

    /// Throw a TypeError with a UTF-8 message.
    pub fn sm_throw_type_error(cx: *mut JSContext, msg: *const u8, msg_len: u32);

    /// Print the current pending exception to stderr.
    pub fn sm_print_pending_exception(cx: *mut JSContext);
}

// ── Debug/introspection ──────────────────────────────────────────────────────

extern "C" {
    /// Convert a value to its source representation string.
    /// Returns a JSString* or null on failure.
    pub fn sm_value_to_source(cx: *mut JSContext, val: JSVal) -> *mut JSString;

    /// Capture the current JS stack as a string.
    /// The caller must free the returned buffer with sm_free().
    /// Returns null if no stack is available.
    pub fn sm_capture_stack_string(cx: *mut JSContext, out_len: *mut u32) -> *mut u8;
}

// ── MapObject operations (for module registry) ───────────────────────────────

extern "C" {
    /// Create a new JS Map object. Returns a persistent root handle, or -1.
    pub fn sm_new_map(cx: *mut JSContext) -> i32;

    /// Check if a map contains a key.
    pub fn sm_map_has(cx: *mut JSContext, map_handle: i32, key: JSVal) -> bool;

    /// Get a value from a map. Stores result in out_val. Returns false if key not found.
    pub fn sm_map_get(cx: *mut JSContext, map_handle: i32, key: JSVal, out_val: *mut JSVal)
        -> bool;

    /// Set a key-value pair in a map.
    pub fn sm_map_set(cx: *mut JSContext, map_handle: i32, key: JSVal, value: JSVal) -> bool;
}

// ── SetObject operations (for promise rejection tracking) ────────────────────

extern "C" {
    /// Create a new JS Set object. Returns a persistent root handle, or -1.
    pub fn sm_new_set(cx: *mut JSContext) -> i32;

    /// Add a value to a set.
    pub fn sm_set_add(cx: *mut JSContext, set_handle: i32, val: JSVal) -> bool;

    /// Remove a value from a set.
    pub fn sm_set_delete(cx: *mut JSContext, set_handle: i32, val: JSVal) -> bool;

    /// Iterate over a set, calling the callback for each element.
    pub fn sm_set_for_each(
        cx: *mut JSContext,
        set_handle: i32,
        callback: Option<unsafe extern "C" fn(val: JSVal, data: *mut c_void) -> bool>,
        data: *mut c_void,
    ) -> bool;

    /// Clear all entries from a set.
    pub fn sm_set_clear(cx: *mut JSContext, set_handle: i32);

    /// Get the number of entries in a set.
    pub fn sm_set_size(cx: *mut JSContext, set_handle: i32) -> u32;
}

// ── Exception setting ────────────────────────────────────────────────────────

extern "C" {
    /// Set a pending exception on the context.
    pub fn sm_set_pending_exception(cx: *mut JSContext, val: JSVal);
}

// ── Global creation variants ─────────────────────────────────────────────────

extern "C" {
    /// Create a new global in the same compartment as an existing global.
    /// Enables streams and does NOT fire OnNewGlobalHook.
    /// Returns a persistent root handle or -1 on failure.
    pub fn sm_new_global_same_compartment(
        cx: *mut JSContext,
        existing_global: *mut JSObject,
    ) -> i32;
}

// ── GC extensions ────────────────────────────────────────────────────────────

extern "C" {
    /// Run a shrinking GC (used after script compilation during pre-init).
    pub fn sm_gc_shrink(cx: *mut JSContext);

    /// Reset the Math.random seed (used after wizer pre-initialization).
    pub fn sm_reset_math_random_seed(cx: *mut JSContext);

    /// Fix Math.random on a global to use a custom random function.
    pub fn sm_fix_math_random(
        cx: *mut JSContext,
        global: *mut JSObject,
        random_fn: Option<unsafe extern "C" fn(*mut JSContext, u32, *mut JSVal) -> bool>,
    ) -> bool;
}

// ── JS CallArgs helpers (for Rust native functions) ──────────────────────────

extern "C" {
    /// Get an argument value from a JS native function's vp.
    pub fn sm_call_args_get(argc: u32, vp: *mut JSVal, index: u32) -> JSVal;

    /// Set the return value of a JS native function to undefined.
    pub fn sm_call_args_rval_set_undefined(argc: u32, vp: *mut JSVal);

    /// Set the return value of a JS native function.
    pub fn sm_call_args_rval_set(argc: u32, vp: *mut JSVal, val: JSVal);
}

// ── Object creation ──────────────────────────────────────────────────────────

extern "C" {
    /// Create a plain JS object (JS_NewPlainObject).
    pub fn sm_new_plain_object(cx: *mut JSContext) -> *mut JSObject;

    /// Allocate a persistent root for a JSObject, returning a handle.
    /// Returns -1 if obj is null.
    pub fn sm_alloc_persistent_root(cx: *mut JSContext, obj: *mut JSObject) -> i32;
}

// ── Property key enumeration ─────────────────────────────────────────────────

extern "C" {
    /// Get own property names of an object as strings.
    /// Returns an array of JSString* via out_strings/out_count.
    /// Caller must free the array with sm_free().
    pub fn sm_get_own_property_names(
        cx: *mut JSContext,
        obj: *mut JSObject,
        out_strings: *mut *mut *mut JSString,
        out_count: *mut u32,
    ) -> bool;
}
