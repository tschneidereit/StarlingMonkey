//! ES module loading and resolution.
//!
//! Replaces `runtime/script_loader.cpp`. Handles module compilation, caching,
//! path resolution, and builtin module synthesis.

use core::ffi::c_void;
use starling_sm_sys as sm;

/// The script/module loader.
///
/// Maintains a module registry (`JS::MapObject`) mapping resolved paths to
/// compiled module objects, and a builtin modules map for `import.meta.builtin`.
pub struct ScriptLoader {
    cx: *mut sm::JSContext,
    /// Persistent root handle for the module registry map.
    module_registry_handle: i32,
    /// Persistent root handle for the builtin modules map.
    builtin_modules_handle: i32,
}

impl ScriptLoader {
    /// Create a new ScriptLoader and register module hooks.
    pub fn new(cx: *mut sm::JSContext) -> Result<Self, &'static str> {
        let module_registry_handle = unsafe { sm::sm_new_map(cx) };
        if module_registry_handle < 0 {
            return Err("Failed to create module registry");
        }

        let builtin_modules_handle = unsafe { sm::sm_new_map(cx) };
        if builtin_modules_handle < 0 {
            return Err("Failed to create builtin modules map");
        }

        let loader = ScriptLoader {
            cx,
            module_registry_handle,
            builtin_modules_handle,
        };

        // Register module resolve/metadata hooks
        unsafe {
            sm::sm_set_module_resolve_hook(cx, Some(module_resolve_hook));
            sm::sm_set_module_metadata_hook(cx, Some(module_metadata_hook));
        }

        Ok(loader)
    }

    /// Register a builtin module (e.g., from a C++ builtin's install() function).
    pub fn define_builtin_module(&self, name: &str, value: sm::JSVal) -> bool {
        let key = unsafe {
            sm::sm_new_string_utf8(self.cx, name.as_ptr(), name.len() as u32)
        };
        if key.is_null() {
            return false;
        }
        let key_val = unsafe { sm::sm_string_value(key) };
        unsafe { sm::sm_map_set(self.cx, self.builtin_modules_handle, key_val, value) }
    }

    /// Evaluate the top-level script.
    ///
    /// If `module_mode` is true, compiles and evaluates as an ES module.
    /// Otherwise, compiles and evaluates as a classic script.
    pub fn eval_top_level_script(
        &self,
        source: &[u8],
        path: &str,
        module_mode: bool,
        global: *mut sm::JSObject,
    ) -> Result<sm::JSVal, &'static str> {
        if module_mode {
            self.eval_as_module(source, path)
        } else {
            self.eval_as_script(source, path, global)
        }
    }

    fn eval_as_module(&self, source: &[u8], path: &str) -> Result<sm::JSVal, &'static str> {
        let module_handle = unsafe {
            sm::sm_compile_module(
                self.cx,
                source.as_ptr(),
                source.len() as u32,
                path.as_ptr(),
                path.len() as u32,
            )
        };
        if module_handle < 0 {
            return Err("Failed to compile module");
        }

        // Cache in registry
        let key = unsafe {
            sm::sm_new_string_utf8(self.cx, path.as_ptr(), path.len() as u32)
        };
        if !key.is_null() {
            let key_val = unsafe { sm::sm_string_value(key) };
            let module_obj = unsafe { sm::sm_get_persistent_rooted(module_handle) };
            if !module_obj.is_null() {
                let module_val = unsafe { sm::sm_object_value(module_obj) };
                unsafe { sm::sm_map_set(self.cx, self.module_registry_handle, key_val, module_val) };
            }
        }

        // Set module private to the path (for resolve hook)
        let path_str = unsafe {
            sm::sm_new_string_utf8(self.cx, path.as_ptr(), path.len() as u32)
        };
        if !path_str.is_null() {
            let module_obj = unsafe { sm::sm_get_persistent_rooted(module_handle) };
            let private_val = unsafe { sm::sm_string_value(path_str) };
            unsafe { sm::sm_set_module_private(module_obj, private_val) };
        }

        if !unsafe { sm::sm_module_link(self.cx, module_handle) } {
            return Err("Failed to link module");
        }

        let eval_promise_handle = unsafe { sm::sm_module_evaluate(self.cx, module_handle) };
        if eval_promise_handle < 0 {
            return Err("Failed to evaluate module");
        }

        let promise_obj = unsafe { sm::sm_get_persistent_rooted(eval_promise_handle) };
        Ok(unsafe { sm::sm_object_value(promise_obj) })
    }

    fn eval_as_script(
        &self,
        source: &[u8],
        path: &str,
        global: *mut sm::JSObject,
    ) -> Result<sm::JSVal, &'static str> {
        let mut result: sm::JSVal = sm::JSVAL_UNDEFINED;
        let ok = unsafe {
            sm::sm_evaluate_script(
                self.cx,
                global,
                source.as_ptr(),
                source.len() as u32,
                path.as_ptr(),
                path.len() as u32,
                &mut result,
            )
        };
        if !ok {
            return Err("Failed to evaluate script");
        }
        Ok(result)
    }

    /// Resolve a module specifier relative to a referencing module's path.
    pub fn resolve_path(specifier: &str, referencing_path: &str) -> String {
        if specifier.starts_with("./") || specifier.starts_with("../") {
            // Relative path — resolve against parent directory
            let base = if let Some(pos) = referencing_path.rfind('/') {
                &referencing_path[..pos + 1]
            } else {
                "./"
            };
            let mut resolved = format!("{base}{specifier}");
            // Normalize path (collapse . and ..)
            resolved = normalize_path(&resolved);
            // Auto-append .js if missing
            if !resolved.contains('.') || resolved.ends_with('/') {
                resolved.push_str(".js");
            }
            resolved
        } else {
            // Bare specifier — could be a builtin module name
            specifier.to_string()
        }
    }
}

impl Drop for ScriptLoader {
    fn drop(&mut self) {
        if self.module_registry_handle >= 0 {
            unsafe { sm::sm_drop_persistent_rooted(self.module_registry_handle) };
        }
        if self.builtin_modules_handle >= 0 {
            unsafe { sm::sm_drop_persistent_rooted(self.builtin_modules_handle) };
        }
    }
}

/// Normalize a path by resolving `.` and `..` components.
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let mut result = parts.join("/");
    if path.starts_with('/') {
        result.insert(0, '/');
    }
    if !path.starts_with('/') && !result.starts_with('.') {
        result.insert_str(0, "./");
    }
    result
}

/// Global ScriptLoader pointer, set during engine init.
static mut SCRIPT_LOADER: *mut ScriptLoader = core::ptr::null_mut();

/// Module resolve hook — called by SpiderMonkey when it encounters an import.
///
/// This is the `extern "C"` callback registered with `sm_set_module_resolve_hook`.
unsafe extern "C" fn module_resolve_hook(
    cx: *mut sm::JSContext,
    referencing_private: sm::JSVal,
    specifier: *mut sm::JSString,
) -> i32 {
    if SCRIPT_LOADER.is_null() {
        return -1;
    }

    // Get specifier as UTF-8
    let mut spec_len: u32 = 0;
    let spec_bytes = sm::sm_encode_string_to_utf8(cx, specifier, &mut spec_len);
    if spec_bytes.is_null() {
        return -1;
    }
    let spec_str = core::str::from_utf8(core::slice::from_raw_parts(spec_bytes, spec_len as usize))
        .unwrap_or("");

    // Get referencing path from private value
    let ref_path = if sm::sm_value_is_string(referencing_private) {
        let ref_str = sm::sm_value_to_string(referencing_private);
        let mut ref_len: u32 = 0;
        let ref_bytes = sm::sm_encode_string_to_utf8(cx, ref_str, &mut ref_len);
        if !ref_bytes.is_null() {
            let s = core::str::from_utf8(core::slice::from_raw_parts(
                ref_bytes,
                ref_len as usize,
            ))
            .unwrap_or("./")
            .to_string();
            sm::sm_free(ref_bytes as *mut c_void);
            s
        } else {
            "./".to_string()
        }
    } else {
        "./".to_string()
    };

    let resolved = ScriptLoader::resolve_path(spec_str, &ref_path);
    sm::sm_free(spec_bytes as *mut c_void);

    // Check if already cached in the module registry
    let loader = &*SCRIPT_LOADER;
    let key = sm::sm_new_string_utf8(cx, resolved.as_ptr(), resolved.len() as u32);
    if key.is_null() {
        return -1;
    }
    let key_val = sm::sm_string_value(key);

    if sm::sm_map_has(cx, loader.module_registry_handle, key_val) {
        // Already compiled — return from cache
        let mut cached_val: sm::JSVal = sm::JSVAL_UNDEFINED;
        if sm::sm_map_get(cx, loader.module_registry_handle, key_val, &mut cached_val) {
            let obj = sm::sm_value_to_object(cached_val);
            if !obj.is_null() {
                // We need to return a persistent root handle, but the cached object
                // is already rooted via the map. For SM's hook, we need to return
                // the JSObject* directly (the hook expects a handle).
                // TODO: Revisit this — may need a temporary persistent root.
                return -1; // placeholder
            }
        }
    }

    // Read the file from the filesystem
    // TODO: Implement file reading via WASI filesystem API
    // For now, return -1 to indicate failure
    -1
}

/// Module metadata hook — populates import.meta for each module.
///
/// Sets `import.meta.builtin` to allow access to builtin modules.
unsafe extern "C" fn module_metadata_hook(
    _cx: *mut sm::JSContext,
    _module_private: sm::JSVal,
    _meta_object: *mut sm::JSObject,
) -> bool {
    // TODO: Populate import.meta.builtin
    true
}

// ── FFI export for C++ builtins ──────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn starling_engine_define_builtin_module(
    id: *const u8,
    id_len: u32,
    value: sm::JSVal,
) -> bool {
    if SCRIPT_LOADER.is_null() || id.is_null() {
        return false;
    }
    let name = core::str::from_utf8(core::slice::from_raw_parts(id, id_len as usize))
        .unwrap_or("");
    (*SCRIPT_LOADER).define_builtin_module(name, value)
}
