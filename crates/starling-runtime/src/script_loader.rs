//! ES module loading and resolution.
//!
//! Full Rust port of `runtime/script_loader.cpp`. Handles module compilation,
//! caching, path resolution, builtin module synthesis, and file loading.

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
    /// Whether to evaluate scripts as ES modules (true) or classic scripts (false).
    module_mode: bool,
    /// Base path for resolving the first script load.
    base_path: String,
    /// Optional path prefix to strip from file names for nicer stack traces.
    path_prefix: Option<String>,
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
            module_mode: true,
            base_path: String::new(),
            path_prefix: None,
        };

        // Register module resolve/metadata hooks
        unsafe {
            sm::sm_set_module_resolve_hook(cx, Some(module_resolve_hook));
            sm::sm_set_module_metadata_hook(cx, Some(module_metadata_hook));
        }

        Ok(loader)
    }

    /// Set the path prefix to strip from filenames in stack traces.
    pub fn set_path_prefix(&mut self, prefix: Option<String>) {
        self.path_prefix = prefix;
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

    /// Set whether to use ES module mode.
    pub fn set_module_mode(&mut self, module_mode: bool) {
        self.module_mode = module_mode;
    }

    /// Get whether module mode is enabled.
    pub fn module_mode(&self) -> bool {
        self.module_mode
    }

    /// Strip the path prefix for nicer display in stack traces.
    fn strip_prefix<'a>(&self, resolved_path: &'a str) -> &'a str {
        if let Some(ref prefix) = self.path_prefix {
            resolved_path.strip_prefix(prefix.as_str()).unwrap_or(resolved_path)
        } else {
            resolved_path
        }
    }

    /// Load a script file. On the first call, determines the base path from
    /// the given path. Subsequent loads resolve relative to that base.
    pub fn load_script(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let resolved = if self.base_path.is_empty() {
            // First load — extract base path
            if let Some(pos) = path.rfind('/') {
                self.base_path = path[..pos + 1].to_string();
            } else {
                self.base_path = "./".to_string();
            }
            path.to_string()
        } else {
            resolve_path(path, &self.base_path)
        };
        self.load_resolved_script(path, &resolved)
    }

    /// Load a script from a resolved file path.
    fn load_resolved_script(&self, specifier: &str, resolved_path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(resolved_path).map_err(|e| {
            format!(
                "Error loading module \"{}\" (resolved path \"{}\"): {}",
                specifier, resolved_path, e
            )
        })
    }

    /// Compile source into a module, set its private metadata, and cache it
    /// in the module registry.
    ///
    /// Delegates to a C++ shim (`sm_compile_and_register_module`) that keeps
    /// all intermediate GC things properly rooted via `Rooted<>`.
    ///
    /// Returns the persistent root handle for the module, or -1 on failure.
    fn compile_and_cache_module(
        &self,
        source: &[u8],
        resolved_path: &str,
    ) -> i32 {
        let cx = self.cx;
        let display_path = self.strip_prefix(resolved_path);

        unsafe {
            sm::sm_compile_and_register_module(
                cx,
                source.as_ptr(),
                source.len() as u32,
                display_path.as_ptr(),
                display_path.len() as u32,
                resolved_path.as_ptr(),
                resolved_path.len() as u32,
                self.module_registry_handle,
            )
        }
    }

    /// Get or compile a module for a specifier + resolved path.
    ///
    /// Returns the persistent root handle for the module, or -1.
    fn get_or_compile_module(
        &self,
        specifier: &str,
        resolved_path: &str,
    ) -> i32 {
        let cx = self.cx;

        // Check cache first — root the key string so it survives any GC
        // triggered by sm_map_get or other SM API calls.
        let key_raw = unsafe {
            sm::sm_new_string_utf8(cx, resolved_path.as_ptr(), resolved_path.len() as u32)
        };
        if key_raw.is_null() {
            return -1;
        }
        rooted_string!(in(cx) let key = key_raw);
        let key_val = unsafe { sm::sm_string_value(key.get()) };

        // Root the lookup result value so the module object pointer
        // stays valid even if a minor GC promotes nursery objects.
        rooted_value!(in(cx) let mut cached_val = sm::JSVAL_UNDEFINED);
        let mut cached_raw: sm::JSVal = sm::JSVAL_UNDEFINED;
        if unsafe { sm::sm_map_get(cx, self.module_registry_handle, key_val, &mut cached_raw) }
            && unsafe { !sm::sm_value_is_undefined(cached_raw) }
        {
            cached_val.set(cached_raw);
            // Already compiled — return a persistent root for the cached object
            let obj = unsafe { sm::sm_value_to_object(cached_val.get()) };
            if !obj.is_null() {
                return unsafe { sm::sm_alloc_persistent_root(cx, obj) };
            }
        }

        // Not cached — load and compile
        let source = match self.load_resolved_script(specifier, resolved_path) {
            Ok(s) => s,
            Err(e) => {
                // Throw a JS error
                let msg = format!("{e}\0");
                unsafe { sm::sm_report_error(cx, msg.as_ptr(), msg.len() as u32 - 1) };
                return -1;
            }
        };

        self.compile_and_cache_module(&source, resolved_path)
    }

    /// Create a builtin module shim that re-exports all properties of a
    /// builtin object via `import.meta.builtin`.
    ///
    /// Generates source like:
    /// ```js
    /// const { 'prop1': e0, 'prop2': e1 } = import.meta.builtin;
    /// export { e0 as 'prop1', e1 as 'prop2' }
    /// ```
    fn get_builtin_module(
        &self,
        id_str: *mut sm::JSString,
        builtin_obj: *mut sm::JSObject,
    ) -> i32 {
        let cx = self.cx;

        // Root the builtin object — sm_map_get below can trigger GC which
        // would leave the raw builtin_obj pointer stale.
        rooted_object!(in(cx) let builtin_rooted = builtin_obj);

        // Check if already cached
        let id_val = unsafe { sm::sm_string_value(id_str) };
        rooted_value!(in(cx) let mut cached_val = sm::JSVAL_UNDEFINED);
        let mut cached_raw: sm::JSVal = sm::JSVAL_UNDEFINED;
        if unsafe { sm::sm_map_get(cx, self.module_registry_handle, id_val, &mut cached_raw) }
            && unsafe { !sm::sm_value_is_undefined(cached_raw) }
        {
            cached_val.set(cached_raw);
            let obj = unsafe { sm::sm_value_to_object(cached_val.get()) };
            if !obj.is_null() {
                return unsafe { sm::sm_alloc_persistent_root(cx, obj) };
            }
        }

        // Enumerate the builtin object's properties — use rooted pointer
        let mut prop_strings: *mut *mut sm::JSString = core::ptr::null_mut();
        let mut prop_count: u32 = 0;
        if !unsafe {
            sm::sm_get_own_property_names(cx, builtin_rooted.get(), &mut prop_strings, &mut prop_count)
        } {
            return -1;
        }

        // Build the shim module source
        let mut code = String::from("const { ");
        let mut export_code = String::new();

        for i in 0..prop_count {
            let prop_str = unsafe { *prop_strings.add(i as usize) };
            let mut prop_len: u32 = 0;
            let prop_bytes = unsafe { sm::sm_encode_string_to_utf8(cx, prop_str, &mut prop_len) };
            if prop_bytes.is_null() {
                unsafe { sm::sm_free(prop_strings as *mut c_void) };
                return -1;
            }
            let prop_name = unsafe {
                core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                    prop_bytes,
                    prop_len as usize,
                ))
            };

            if i > 0 {
                code.push_str(", ");
                export_code.push_str(", ");
            }

            // Destructuring: 'propName': eN
            code.push('\'');
            code.push_str(prop_name);
            code.push_str("': e");
            code.push_str(&i.to_string());

            // Export: eN as 'propName'
            export_code.push('e');
            export_code.push_str(&i.to_string());
            export_code.push_str(" as '");
            export_code.push_str(prop_name);
            export_code.push('\'');

            unsafe { sm::sm_free(prop_bytes as *mut c_void) };
        }
        unsafe { sm::sm_free(prop_strings as *mut c_void) };

        code.push_str(" } = import.meta.builtin;\nexport { ");
        code.push_str(&export_code);
        code.push_str(" }\n");

        // Compile the shim module
        let filename = b"<internal>";
        let module_handle = unsafe {
            sm::sm_compile_module(
                cx,
                code.as_ptr(),
                code.len() as u32,
                filename.as_ptr(),
                filename.len() as u32,
            )
        };
        if module_handle < 0 {
            return -1;
        }

        // Set module private and cache in registry — done in C++ with proper
        // Rooted<> for all intermediate GC things.
        unsafe {
            sm::sm_register_module(cx, module_handle, id_val, self.module_registry_handle)
        }
    }

    /// Evaluate the top-level script.
    ///
    /// If `module_mode` is true, compiles as an ES module, links, evaluates,
    /// and returns the evaluation promise. Otherwise evaluates as a classic script.
    pub fn eval_top_level_script(
        &mut self,
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

    fn eval_as_module(&mut self, source: &[u8], path: &str) -> Result<sm::JSVal, &'static str> {
        let cx = self.cx;

        // Set base_path from the first load if not set
        if self.base_path.is_empty() {
            if let Some(pos) = path.rfind('/') {
                self.base_path = path[..pos + 1].to_string();
            } else {
                self.base_path = "./".to_string();
            }
        }

        // Compile the module
        let module_handle = self.compile_and_cache_module(source, path);
        if module_handle < 0 {
            return Err("Failed to compile module");
        }

        // Link the module (resolve imports)
        if !unsafe { sm::sm_module_link(cx, module_handle) } {
            return Err("Failed to link module");
        }

        // Shrinking GC before evaluation during pre-init — compacts the heap
        // to reduce pages touched post-deploy.
        let engine_state = unsafe { crate::engine::Engine::get().state() };
        if engine_state == crate::engine::EngineState::ScriptPreInitializing {
            unsafe {
                sm::sm_gc_shrink(cx);
            }
        }

        // Evaluate the module (returns the evaluation promise)
        let eval_promise_handle = unsafe { sm::sm_module_evaluate(cx, module_handle) };
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
        let display_path = self.strip_prefix(path);
        let mut result: sm::JSVal = sm::JSVAL_UNDEFINED;
        let ok = unsafe {
            sm::sm_evaluate_script(
                self.cx,
                global,
                source.as_ptr(),
                source.len() as u32,
                display_path.as_ptr(),
                display_path.len() as u32,
                &mut result,
            )
        };
        if !ok {
            return Err("Failed to evaluate script");
        }
        Ok(result)
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

// ── Path resolution ──────────────────────────────────────────────────────────

/// Resolve a file path relative to a base path, handling `.` and `..` segments.
///
/// Faithful port of the C++ `resolve_path` function.
fn resolve_path(path: &str, base: &str) -> String {
    // Find the directory part of the base path
    let base_dir = if let Some(pos) = base.rfind('/') {
        &base[..pos + 1]
    } else {
        ""
    };

    let mut resolved = String::with_capacity(base_dir.len() + path.len() + 1);

    if path.starts_with('/') {
        // Absolute path — ignore base
    } else {
        resolved.push_str(base_dir);
    }

    // Process path segments
    let segments = path.as_bytes();
    let mut from = 0;
    let mut cur = 0;

    while cur < segments.len() {
        // Advance to the next '/' or end
        while cur < segments.len() && segments[cur] != b'/' {
            cur += 1;
        }
        if cur == from {
            break;
        }

        let segment = &segments[from..cur];

        if segment == b"." {
            // Skip '.' segments
        } else if segment == b".." {
            // Backtrack one directory
            if resolved.ends_with('/') {
                resolved.pop();
            }
            if let Some(pos) = resolved.rfind('/') {
                resolved.truncate(pos + 1);
            } else {
                resolved.clear();
            }
        } else {
            // Normal segment — append it
            resolved.push_str(core::str::from_utf8(segment).unwrap_or(""));
            if cur < segments.len() && segments[cur] == b'/' {
                resolved.push('/');
            }
        }

        // Skip the '/' separator
        if cur < segments.len() && segments[cur] == b'/' {
            cur += 1;
        }
        from = cur;
    }

    resolve_extension(resolved)
}

/// If the resolved path doesn't exist, try appending ".js".
fn resolve_extension(resolved: String) -> String {
    if file_exists(&resolved) {
        return resolved;
    }

    if resolved.len() >= 3 && resolved.ends_with(".js") {
        return resolved;
    }

    let with_ext = format!("{resolved}.js");
    if file_exists(&with_ext) {
        return with_ext;
    }

    resolved
}

/// Check if a file exists.
fn file_exists(path: &str) -> bool {
    std::fs::metadata(path).is_ok()
}

// ── Module hooks ─────────────────────────────────────────────────────────────

/// Module resolve hook — called by SpiderMonkey when it encounters an import.
///
/// Returns a persistent root handle for the resolved module, or -1 on failure.
unsafe extern "C" fn module_resolve_hook(
    cx: *mut sm::JSContext,
    referencing_private: sm::JSVal,
    specifier: *mut sm::JSString,
) -> i32 {
    let engine = crate::engine::Engine::get();
    let loader = match engine.script_loader() {
        Some(l) => l,
        None => return -1,
    };

    // Root the referencing private value — it's a copy of the JSVal from
    // the C++ lambda's HandleValue. SM API calls below can trigger GC which
    // would leave this copy stale if the contained object moves.
    rooted_value!(in(cx) let private_rooted = referencing_private);

    // Get specifier as UTF-8
    let mut spec_len: u32 = 0;
    let spec_bytes = sm::sm_encode_string_to_utf8(cx, specifier, &mut spec_len);
    if spec_bytes.is_null() {
        return -1;
    }
    let spec_str = match core::str::from_utf8(core::slice::from_raw_parts(
        spec_bytes,
        spec_len as usize,
    )) {
        Ok(s) => s,
        Err(_) => {
            sm::sm_free(spec_bytes as *mut c_void);
            return -1;
        }
    };

    // Check if it's a builtin module
    let specifier_val = sm::sm_string_value(specifier);
    rooted_value!(in(cx) let mut builtin_val = sm::JSVAL_UNDEFINED);
    let mut builtin_raw: sm::JSVal = sm::JSVAL_UNDEFINED;
    if sm::sm_map_get(cx, loader.builtin_modules_handle, specifier_val, &mut builtin_raw)
        && !sm::sm_value_is_undefined(builtin_raw)
    {
        builtin_val.set(builtin_raw);
        sm::sm_free(spec_bytes as *mut c_void);
        let builtin_obj = sm::sm_value_to_object(builtin_val.get());
        return loader.get_builtin_module(specifier, builtin_obj);
    }

    // Get the referencing module's path from its private value
    // Use the rooted copy to ensure the object pointer is up-to-date.
    let parent_path = get_module_path(cx, private_rooted.get());

    // Resolve the specifier relative to the parent path
    let spec_owned = spec_str.to_string();
    sm::sm_free(spec_bytes as *mut c_void);
    let resolved = resolve_path(&spec_owned, &parent_path);

    // Get or compile the module
    loader.get_or_compile_module(&spec_owned, &resolved)
}

/// Extract the module path from a module's private value.
///
/// The private is an object `{id: "path/to/module.js"}`.
unsafe fn get_module_path(cx: *mut sm::JSContext, private_val: sm::JSVal) -> String {
    if !sm::sm_value_is_object(private_val) {
        return String::from("./");
    }
    // Root the info object so it survives any GC triggered by sm_get_property.
    let info_raw = sm::sm_value_to_object(private_val);
    if info_raw.is_null() {
        return String::from("./");
    }
    rooted_object!(in(cx) let info_obj = info_raw);

    let id_name = b"id\0";
    let mut id_val: sm::JSVal = sm::JSVAL_UNDEFINED;
    if !sm::sm_get_property(cx, info_obj.get(), id_name.as_ptr(), 2, &mut id_val) {
        return String::from("./");
    }
    if !sm::sm_value_is_string(id_val) {
        return String::from("./");
    }

    let id_str = sm::sm_value_to_string(id_val);
    let mut len: u32 = 0;
    let bytes = sm::sm_encode_string_to_utf8(cx, id_str, &mut len);
    if bytes.is_null() {
        return String::from("./");
    }

    let result =
        core::str::from_utf8(core::slice::from_raw_parts(bytes, len as usize))
            .unwrap_or("./")
            .to_string();
    sm::sm_free(bytes as *mut c_void);
    result
}

/// Module metadata hook — populates `import.meta` for each module.
///
/// For builtin modules, sets `import.meta.builtin` to the builtin object
/// so the shim module can destructure it.
unsafe extern "C" fn module_metadata_hook(
    cx: *mut sm::JSContext,
    module_private: sm::JSVal,
    meta_object: *mut sm::JSObject,
) -> bool {
    let engine = crate::engine::Engine::get();
    let loader = match engine.script_loader() {
        Some(l) => l,
        None => return false,
    };

    // Root the module private and meta object across SM API calls.
    rooted_value!(in(cx) let private_rooted = module_private);
    rooted_object!(in(cx) let meta_rooted = meta_object);

    // Get the module's id from private
    if !sm::sm_value_is_object(private_rooted.get()) {
        return false;
    }
    let info_raw = sm::sm_value_to_object(private_rooted.get());
    if info_raw.is_null() {
        return false;
    }
    rooted_object!(in(cx) let info_obj = info_raw);

    let id_name = b"id\0";
    rooted_value!(in(cx) let mut id_val = sm::JSVAL_UNDEFINED);
    let mut id_raw: sm::JSVal = sm::JSVAL_UNDEFINED;
    if !sm::sm_get_property(cx, info_obj.get(), id_name.as_ptr(), 2, &mut id_raw) {
        return false;
    }
    id_val.set(id_raw);
    if !sm::sm_value_is_string(id_val.get()) {
        return false;
    }

    // Check if this module's id matches a builtin module
    rooted_value!(in(cx) let mut builtin_val = sm::JSVAL_UNDEFINED);
    let mut builtin_raw: sm::JSVal = sm::JSVAL_UNDEFINED;
    if !sm::sm_map_get(cx, loader.builtin_modules_handle, id_val.get(), &mut builtin_raw) {
        return false;
    }
    builtin_val.set(builtin_raw);
    if sm::sm_value_is_undefined(builtin_val.get()) {
        return false;
    }

    // Set import.meta.builtin = <the builtin object>
    let prop_name = b"builtin\0";
    sm::sm_set_property(cx, meta_rooted.get(), prop_name.as_ptr(), 7, builtin_val.get())
}
