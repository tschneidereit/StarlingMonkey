// SpiderMonkey initialization and context management shim.
//
// Wraps SM's C++ API as extern "C" functions callable from Rust.

#include "jsapi.h"
#include "js/Initialization.h"
#include "js/ContextOptions.h"
#include "js/CompilationAndEvaluation.h"
#include "js/SourceText.h"
#include "js/Modules.h"
#include "js/Promise.h"
#include "js/friend/ErrorMessages.h"
#include "js/Exception.h"
#include "js/MapAndSet.h"
#include "js/Object.h"
#include "js/ForOfIterator.h"
#include "js/PropertyAndElement.h"
#include "js/Conversions.h"
#include "jsfriendapi.h"

#include <cstdlib>
#include <cstring>
#include <vector>

// ── Persistent root storage ──────────────────────────────────────────────────
//
// SM's PersistentRooted types must live in C++ (GC needs to trace them).
// We store them in a simple indexed table and return integer handles to Rust.

static JSContext *g_cx = nullptr;

struct PersistentRootEntry {
    JS::PersistentRootedObject obj;
    bool in_use;
};

static std::vector<PersistentRootEntry*> g_persistent_roots;
static std::vector<int32_t> g_free_list;

static int32_t alloc_persistent_root(JSContext *cx, JSObject *obj) {
    int32_t handle;
    auto *entry = new PersistentRootEntry();
    entry->obj.init(cx, obj);
    entry->in_use = true;

    if (!g_free_list.empty()) {
        handle = g_free_list.back();
        g_free_list.pop_back();
        g_persistent_roots[handle] = entry;
    } else {
        handle = static_cast<int32_t>(g_persistent_roots.size());
        g_persistent_roots.push_back(entry);
    }
    return handle;
}

// ── Initialization & context ─────────────────────────────────────────────────

extern "C" {

bool sm_init() {
    return JS_Init();
}

void sm_shutdown() {
    JS_ShutDown();
}

JSContext *sm_new_context() {
    JSContext *cx = JS_NewContext(JS::DefaultHeapMaxBytes);
    if (!cx) return nullptr;
    g_cx = cx;
    return cx;
}

void sm_destroy_context(JSContext *cx) {
    // Clean up persistent roots
    for (auto *entry : g_persistent_roots) {
        if (entry) {
            delete entry;
        }
    }
    g_persistent_roots.clear();
    g_free_list.clear();
    g_cx = nullptr;
    JS_DestroyContext(cx);
}

bool sm_init_self_hosted_code(JSContext *cx) {
    return JS::InitSelfHostedCode(cx);
}

bool sm_use_internal_job_queues(JSContext *cx) {
    return js::UseInternalJobQueues(cx);
}

void sm_set_pbl_enabled(JSContext *cx, bool enabled) {
    JS_SetGlobalJitCompilerOption(
        cx, JSJITCOMPILER_PORTABLE_BASELINE_ENABLE,
        enabled ? 1 : 0);
}

void sm_set_context_private(JSContext *cx, void *data) {
    JS_SetContextPrivate(cx, data);
}

void *sm_get_context_private(JSContext *cx) {
    return JS_GetContextPrivate(cx);
}

} // extern "C"

// ── Globals & realms ─────────────────────────────────────────────────────────

static JSClassOps g_global_class_ops = {};
static JSClass g_global_class = {
    "global",
    JSCLASS_GLOBAL_FLAGS,
    &g_global_class_ops
};

extern "C" {

int32_t sm_new_global(JSContext *cx) {
    JS::RealmOptions options;
    options.creationOptions().setStreamsEnabled(true);
    JS::RootedObject global(cx, JS_NewGlobalObject(cx, &g_global_class, nullptr,
                                                    JS::FireOnNewGlobalHook, options));
    if (!global) return -1;

    JSAutoRealm ar(cx, global);
    if (!JS::InitRealmStandardClasses(cx)) return -1;

    return alloc_persistent_root(cx, global);
}

JSObject *sm_get_persistent_rooted(int32_t handle) {
    if (handle < 0 || handle >= static_cast<int32_t>(g_persistent_roots.size()))
        return nullptr;
    auto *entry = g_persistent_roots[handle];
    if (!entry || !entry->in_use) return nullptr;
    return entry->obj.get();
}

void sm_drop_persistent_rooted(int32_t handle) {
    if (handle < 0 || handle >= static_cast<int32_t>(g_persistent_roots.size()))
        return;
    auto *entry = g_persistent_roots[handle];
    if (!entry) return;
    delete entry;
    g_persistent_roots[handle] = nullptr;
    g_free_list.push_back(handle);
}

void *sm_enter_realm(JSContext *cx, JSObject *global) {
    // Returns the old realm compartment as an opaque pointer.
    // This is safe because JSAutoRealm just wraps a realm enter/leave.
    return reinterpret_cast<void*>(JS::EnterRealm(cx, global));
}

void sm_leave_realm(JSContext *cx, void *old_realm) {
    JS::LeaveRealm(cx, reinterpret_cast<JS::Realm*>(old_realm));
}

bool sm_define_function(
    JSContext *cx, JSObject *obj,
    const uint8_t *name, uint32_t name_len,
    bool (*func)(JSContext*, unsigned, JS::Value*),
    uint32_t nargs, uint32_t flags
) {
    // Create null-terminated name
    std::string name_str(reinterpret_cast<const char*>(name), name_len);

    JS::RootedObject robj(cx, obj);
    JSFunction *jsfn = JS_DefineFunction(cx, robj, name_str.c_str(),
                                          reinterpret_cast<JSNative>(func),
                                          nargs, flags);
    return jsfn != nullptr;
}

bool sm_define_property_value(
    JSContext *cx, JSObject *obj,
    const uint8_t *name, uint32_t name_len,
    JS::Value value, uint32_t flags
) {
    std::string name_str(reinterpret_cast<const char*>(name), name_len);
    JS::RootedObject robj(cx, obj);
    JS::RootedValue rval(cx, value);
    return JS_DefineProperty(cx, robj, name_str.c_str(), rval, flags);
}

} // extern "C"

// ── Script compilation & evaluation ──────────────────────────────────────────

extern "C" {

bool sm_evaluate_script(
    JSContext *cx, JSObject *global,
    const uint8_t *source, uint32_t source_len,
    const uint8_t *filename, uint32_t filename_len,
    JS::Value *result
) {
    JS::RootedObject rglobal(cx, global);
    JSAutoRealm ar(cx, rglobal);

    JS::SourceText<mozilla::Utf8Unit> srcBuf;
    if (!srcBuf.init(cx, reinterpret_cast<const char*>(source), source_len,
                     JS::SourceOwnership::Borrowed)) {
        return false;
    }

    std::string fname(reinterpret_cast<const char*>(filename), filename_len);
    JS::CompileOptions opts(cx);
    opts.setFileAndLine(fname.c_str(), 1);

    JS::RootedValue rval(cx);
    if (!JS::Evaluate(cx, opts, srcBuf, &rval)) {
        return false;
    }
    *result = rval;
    return true;
}

int32_t sm_compile_module(
    JSContext *cx,
    const uint8_t *source, uint32_t source_len,
    const uint8_t *filename, uint32_t filename_len
) {
    JS::SourceText<mozilla::Utf8Unit> srcBuf;
    if (!srcBuf.init(cx, reinterpret_cast<const char*>(source), source_len,
                     JS::SourceOwnership::Borrowed)) {
        return -1;
    }

    std::string fname(reinterpret_cast<const char*>(filename), filename_len);
    JS::CompileOptions opts(cx);
    opts.setFileAndLine(fname.c_str(), 1);

    JS::RootedObject module(cx, JS::CompileModule(cx, opts, srcBuf));
    if (!module) return -1;

    return alloc_persistent_root(cx, module);
}

bool sm_module_link(JSContext *cx, int32_t module_handle) {
    JSObject *module = sm_get_persistent_rooted(module_handle);
    if (!module) return false;
    JS::RootedObject rmodule(cx, module);
    return JS::ModuleLink(cx, rmodule);
}

int32_t sm_module_evaluate(JSContext *cx, int32_t module_handle) {
    JSObject *module = sm_get_persistent_rooted(module_handle);
    if (!module) return -1;
    JS::RootedObject rmodule(cx, module);
    JS::RootedValue rval(cx);
    if (!JS::ModuleEvaluate(cx, rmodule, &rval)) {
        return -1;
    }
    // rval is the evaluation promise
    if (!rval.isObject()) return -1;
    return alloc_persistent_root(cx, &rval.toObject());
}

void sm_set_module_resolve_hook(
    JSContext *cx,
    int32_t (*hook)(JSContext*, JS::Value, JSString*)
) {
    // Store the user's hook in a static so the real SM hook can call it.
    static int32_t (*s_resolve_hook)(JSContext*, JS::Value, JSString*) = nullptr;
    s_resolve_hook = hook;

    if (!hook) {
        JS::SetModuleResolveHook(JS_GetRuntime(cx), nullptr);
        return;
    }

    // The real SM hook receives HandleValue and HandleObject, but we simplify.
    JS::SetModuleResolveHook(JS_GetRuntime(cx),
        [](JSContext *cx, JS::HandleValue referencingPrivate,
           JS::HandleObject moduleRequest) -> JSObject* {
            JS::RootedString specifier(cx, JS::GetModuleRequestSpecifier(cx, moduleRequest));
            if (!specifier) return nullptr;
            int32_t handle = s_resolve_hook(cx, referencingPrivate.get(), specifier);
            if (handle < 0) return nullptr;
            return sm_get_persistent_rooted(handle);
        });
}

void sm_set_module_metadata_hook(
    JSContext *cx,
    bool (*hook)(JSContext*, JS::Value, JSObject*)
) {
    static bool (*s_metadata_hook)(JSContext*, JS::Value, JSObject*) = nullptr;
    s_metadata_hook = hook;

    if (!hook) {
        JS::SetModuleMetadataHook(JS_GetRuntime(cx), nullptr);
        return;
    }

    JS::SetModuleMetadataHook(JS_GetRuntime(cx),
        [](JSContext *cx, JS::HandleValue privateValue,
           JS::HandleObject metaObject) -> bool {
            return s_metadata_hook(cx, privateValue.get(), metaObject);
        });
}

JSString *sm_get_module_request_specifier(JSContext *cx, JSObject *module_request) {
    JS::RootedObject req(cx, module_request);
    return JS::GetModuleRequestSpecifier(cx, req);
}

void sm_set_module_private(JSObject *module, JS::Value private_value) {
    JS::SetModulePrivate(module, private_value);
}

JSObject *sm_get_module_namespace(JSContext *cx, int32_t module_handle) {
    JSObject *module = sm_get_persistent_rooted(module_handle);
    if (!module) return nullptr;
    JS::RootedObject rmodule(cx, module);
    return JS::GetModuleNamespace(cx, rmodule);
}

} // extern "C"

// ── String operations ────────────────────────────────────────────────────────

extern "C" {

uint8_t *sm_encode_string_to_utf8(JSContext *cx, JSString *str, uint32_t *out_len) {
    JS::RootedString rstr(cx, str);
    JS::UniqueChars chars = JS_EncodeStringToUTF8(cx, rstr);
    if (!chars) return nullptr;
    size_t len = strlen(chars.get());
    auto *buf = static_cast<uint8_t*>(malloc(len));
    if (!buf) return nullptr;
    memcpy(buf, chars.get(), len);
    *out_len = static_cast<uint32_t>(len);
    return buf;
}

JSString *sm_new_string_utf8(JSContext *cx, const uint8_t *bytes, uint32_t len) {
    return JS_NewStringCopyUTF8N(cx, JS::UTF8Chars(reinterpret_cast<const char*>(bytes), len));
}

JSString *sm_new_string_latin1(JSContext *cx, const uint8_t *bytes, uint32_t len) {
    // Copy bytes (SM takes ownership after UniqueLatin1Chars)
    auto *copy = static_cast<JS::Latin1Char*>(JS_malloc(cx, len + 1));
    if (!copy) return nullptr;
    memcpy(copy, bytes, len);
    copy[len] = 0;
    return JS_NewLatin1String(cx, JS::UniqueLatin1Chars(copy), len);
}

uint32_t sm_get_string_length(JSString *str) {
    return JS_GetStringLength(str);
}

void sm_free(void *ptr) {
    free(ptr);
}

} // extern "C"

// ── Value operations ─────────────────────────────────────────────────────────

extern "C" {

JS::Value sm_int32_value(int32_t v) {
    return JS::Int32Value(v);
}

JS::Value sm_double_value(double v) {
    return JS::DoubleValue(v);
}

JS::Value sm_string_value(JSString *str) {
    return JS::StringValue(str);
}

JS::Value sm_object_value(JSObject *obj) {
    return JS::ObjectValue(*obj);
}

JS::Value sm_boolean_value(bool v) {
    return JS::BooleanValue(v);
}

JSObject *sm_value_to_object(JS::Value val) {
    if (!val.isObject()) return nullptr;
    return &val.toObject();
}

bool sm_value_is_object(JS::Value val) {
    return val.isObject();
}

bool sm_value_is_undefined(JS::Value val) {
    return val.isUndefined();
}

bool sm_value_is_null(JS::Value val) {
    return val.isNull();
}

bool sm_value_is_string(JS::Value val) {
    return val.isString();
}

JSString *sm_value_to_string(JS::Value val) {
    if (!val.isString()) return nullptr;
    return val.toString();
}

bool sm_get_property(
    JSContext *cx, JSObject *obj,
    const uint8_t *name, uint32_t name_len,
    JS::Value *result
) {
    std::string name_str(reinterpret_cast<const char*>(name), name_len);
    JS::RootedObject robj(cx, obj);
    JS::RootedValue rval(cx);
    if (!JS_GetProperty(cx, robj, name_str.c_str(), &rval)) {
        return false;
    }
    *result = rval;
    return true;
}

bool sm_set_property(
    JSContext *cx, JSObject *obj,
    const uint8_t *name, uint32_t name_len,
    JS::Value value
) {
    std::string name_str(reinterpret_cast<const char*>(name), name_len);
    JS::RootedObject robj(cx, obj);
    JS::RootedValue rval(cx, value);
    return JS_SetProperty(cx, robj, name_str.c_str(), rval);
}

bool sm_call_function(
    JSContext *cx, JSObject *this_obj, JS::Value func,
    uint32_t argc, const JS::Value *argv,
    JS::Value *result
) {
    JS::RootedObject rthis(cx, this_obj);
    JS::RootedValue rfunc(cx, func);
    JS::RootedValue rresult(cx);

    JS::RootedValueVector args(cx);
    if (!args.resize(argc)) return false;
    for (uint32_t i = 0; i < argc; i++) {
        args[i].set(argv[i]);
    }

    JS::HandleValueArray hargs(args);
    if (!JS_CallFunctionValue(cx, rthis, rfunc, hargs, &rresult)) {
        return false;
    }
    *result = rresult;
    return true;
}

} // extern "C"

// ── Promise operations ───────────────────────────────────────────────────────

extern "C" {

void sm_set_promise_rejection_tracker(
    JSContext *cx,
    void (*hook)(JSContext*, JSObject*, uint32_t, void*),
    void *data
) {
    static void (*s_hook)(JSContext*, JSObject*, uint32_t, void*) = nullptr;
    static void *s_data = nullptr;
    s_hook = hook;
    s_data = data;

    if (!hook) {
        JS::SetPromiseRejectionTrackerCallback(cx, nullptr, nullptr);
        return;
    }

    JS::SetPromiseRejectionTrackerCallback(cx,
        [](JSContext *cx, bool mutedErrors, JS::HandleObject promise,
           JS::PromiseRejectionHandlingState state, void *data) {
            uint32_t state_u32 = static_cast<uint32_t>(state);
            s_hook(cx, promise, state_u32, s_data);
        },
        nullptr);
}

uint32_t sm_get_promise_state(JSContext *cx, JSObject *promise) {
    JS::RootedObject rpromise(cx, promise);
    return static_cast<uint32_t>(JS::GetPromiseState(rpromise));
}

uint64_t sm_get_promise_result(JSContext *cx, JSObject *promise) {
    JS::RootedObject rpromise(cx, promise);
    return JS::GetPromiseResult(rpromise).asRawBits();
}

JSObject *sm_new_promise(JSContext *cx) {
    return JS::NewPromiseObject(cx, nullptr);
}

bool sm_resolve_promise(JSContext *cx, JSObject *promise, JS::Value value) {
    JS::RootedObject rpromise(cx, promise);
    JS::RootedValue rval(cx, value);
    return JS::ResolvePromise(cx, rpromise, rval);
}

bool sm_reject_promise(JSContext *cx, JSObject *promise, JS::Value reason) {
    JS::RootedObject rpromise(cx, promise);
    JS::RootedValue rval(cx, reason);
    return JS::RejectPromise(cx, rpromise, rval);
}

} // extern "C"

// ── GC operations ────────────────────────────────────────────────────────────

extern "C" {

void sm_gc(JSContext *cx) {
    JS::PrepareForFullGC(cx);
    JS::NonIncrementalGC(cx, JS::GCOptions::Normal, JS::GCReason::API);
}

bool sm_run_jobs(JSContext *cx) {
    js::RunJobs(cx);
    return true;
}

void sm_add_extra_gc_roots_tracer(
    JSContext *cx,
    void (*tracer)(JSTracer*, void*),
    void *data
) {
    JS_AddExtraGCRootsTracer(cx, tracer, data);
}

} // extern "C"

// ── Errors ───────────────────────────────────────────────────────────────────

extern "C" {

bool sm_is_exception_pending(JSContext *cx) {
    return JS_IsExceptionPending(cx);
}

void sm_clear_pending_exception(JSContext *cx) {
    JS_ClearPendingException(cx);
}

bool sm_steal_pending_exception(JSContext *cx, JS::Value *out_val) {
    JS::ExceptionStack exnStack(cx);
    if (!JS::StealPendingExceptionStack(cx, &exnStack)) {
        return false;
    }
    *out_val = exnStack.exception();
    return true;
}

void sm_report_error(JSContext *cx, const uint8_t *msg, uint32_t msg_len) {
    std::string s(reinterpret_cast<const char*>(msg), msg_len);
    JS_ReportErrorUTF8(cx, "%s", s.c_str());
}

void sm_throw_type_error(JSContext *cx, const uint8_t *msg, uint32_t msg_len) {
    std::string s(reinterpret_cast<const char*>(msg), msg_len);
    JS_ReportErrorUTF8(cx, "TypeError: %s", s.c_str());
}

void sm_print_pending_exception(JSContext *cx) {
    JS::ExceptionStack exnStack(cx);
    if (JS::StealPendingExceptionStack(cx, &exnStack)) {
        JS::ErrorReportBuilder report(cx);
        if (report.init(cx, exnStack, JS::ErrorReportBuilder::NoSideEffects)) {
            JS::PrintError(stderr, report, /* reportWarnings */ false);
        }
    }
}

} // extern "C"

// ── Debug/introspection ──────────────────────────────────────────────────────

extern "C" {

JSString *sm_value_to_source(JSContext *cx, JS::Value val) {
    JS::RootedValue rval(cx, val);
    return JS_ValueToSource(cx, rval);
}

uint8_t *sm_capture_stack_string(JSContext *cx, uint32_t *out_len) {
    JS::RootedObject stack(cx);
    if (!JS::CaptureCurrentStack(cx, &stack) || !stack) {
        *out_len = 0;
        return nullptr;
    }
    JS::RootedString stackStr(cx);
    if (!JS::BuildStackString(cx, nullptr, stack, &stackStr, 0)) {
        *out_len = 0;
        return nullptr;
    }

    JS::UniqueChars chars = JS_EncodeStringToUTF8(cx, stackStr);
    if (!chars) {
        *out_len = 0;
        return nullptr;
    }

    size_t len = strlen(chars.get());
    auto *buf = static_cast<uint8_t*>(malloc(len));
    if (!buf) {
        *out_len = 0;
        return nullptr;
    }
    memcpy(buf, chars.get(), len);
    *out_len = static_cast<uint32_t>(len);
    return buf;
}

} // extern "C"

// ── MapObject operations ─────────────────────────────────────────────────────

extern "C" {

int32_t sm_new_map(JSContext *cx) {
    JSObject *map = JS::NewMapObject(cx);
    if (!map) return -1;
    return alloc_persistent_root(cx, map);
}

bool sm_map_has(JSContext *cx, int32_t map_handle, JS::Value key) {
    JSObject *map = sm_get_persistent_rooted(map_handle);
    if (!map) return false;
    JS::RootedObject rmap(cx, map);
    JS::RootedValue rkey(cx, key);
    bool found = false;
    if (!JS::MapHas(cx, rmap, rkey, &found)) return false;
    return found;
}

bool sm_map_get(JSContext *cx, int32_t map_handle, JS::Value key, JS::Value *out_val) {
    JSObject *map = sm_get_persistent_rooted(map_handle);
    if (!map) return false;
    JS::RootedObject rmap(cx, map);
    JS::RootedValue rkey(cx, key);
    JS::RootedValue rval(cx);
    if (!JS::MapGet(cx, rmap, rkey, &rval)) return false;
    *out_val = rval;
    return true;
}

bool sm_map_set(JSContext *cx, int32_t map_handle, JS::Value key, JS::Value value) {
    JSObject *map = sm_get_persistent_rooted(map_handle);
    if (!map) return false;
    JS::RootedObject rmap(cx, map);
    JS::RootedValue rkey(cx, key);
    JS::RootedValue rval(cx, value);
    return JS::MapSet(cx, rmap, rkey, rval);
}

} // extern "C"

// ── SetObject operations ─────────────────────────────────────────────────────

extern "C" {

int32_t sm_new_set(JSContext *cx) {
    JSObject *set = JS::NewSetObject(cx);
    if (!set) return -1;
    return alloc_persistent_root(cx, set);
}

bool sm_set_add(JSContext *cx, int32_t set_handle, JS::Value val) {
    JSObject *set = sm_get_persistent_rooted(set_handle);
    if (!set) return false;
    JS::RootedObject rset(cx, set);
    JS::RootedValue rval(cx, val);
    return JS::SetAdd(cx, rset, rval);
}

bool sm_set_delete(JSContext *cx, int32_t set_handle, JS::Value val) {
    JSObject *set = sm_get_persistent_rooted(set_handle);
    if (!set) return false;
    JS::RootedObject rset(cx, set);
    JS::RootedValue rval(cx, val);
    bool found = false;
    return JS::SetDelete(cx, rset, rval, &found);
}

bool sm_set_for_each(JSContext *cx, int32_t set_handle,
                     bool (*callback)(JS::Value, void*), void *data) {
    JSObject *set = sm_get_persistent_rooted(set_handle);
    if (!set) return false;

    JS::RootedObject rset(cx, set);
    JS::RootedValue setVal(cx, JS::ObjectValue(*set));

    JS::ForOfIterator iter(cx);
    if (!iter.init(setVal)) return false;

    bool done = false;
    JS::RootedValue val(cx);
    while (true) {
        if (!iter.next(&val, &done)) return false;
        if (done) break;
        if (!callback(val, data)) return false;
    }
    return true;
}

void sm_set_clear(JSContext *cx, int32_t set_handle) {
    JSObject *set = sm_get_persistent_rooted(set_handle);
    if (!set) return;
    JS::RootedObject rset(cx, set);
    JS::SetClear(cx, rset);
}

uint32_t sm_set_size(JSContext *cx, int32_t set_handle) {
    JSObject *set = sm_get_persistent_rooted(set_handle);
    if (!set) return 0;
    JS::RootedObject rset(cx, set);
    return JS::SetSize(cx, rset);
}

} // extern "C"

// ── Exception setting ────────────────────────────────────────────────────────

extern "C" {

void sm_set_pending_exception(JSContext *cx, JS::Value val) {
    JS::RootedValue rval(cx, val);
    JS_SetPendingException(cx, rval);
}

} // extern "C"

// ── Global creation variants ─────────────────────────────────────────────────

extern "C" {

/// Create a new global in the same compartment as an existing global.
/// Enables streams and does NOT fire OnNewGlobalHook.
/// Returns a persistent root handle or -1 on failure.
int32_t sm_new_global_same_compartment(JSContext *cx, JSObject *existing_global) {
    JS::RealmOptions options;
    options.creationOptions()
        .setStreamsEnabled(true)
        .setExistingCompartment(existing_global);

    static JSClass global_class = {
        "global",
        JSCLASS_GLOBAL_FLAGS,
        &JS::DefaultGlobalClassOps
    };

    JS::RootedObject global(cx, JS_NewGlobalObject(cx, &global_class, nullptr,
                                                    JS::DontFireOnNewGlobalHook, options));
    if (!global) return -1;

    JSAutoRealm ar(cx, global);
    if (!JS::InitRealmStandardClasses(cx)) return -1;

    return alloc_persistent_root(cx, global);
}

} // extern "C"

// ── GC extensions ────────────────────────────────────────────────────────────

extern "C" {

/// Run a shrinking GC (used after script compilation during pre-init).
void sm_gc_shrink(JSContext *cx) {
    JS::PrepareForFullGC(cx);
    JS::NonIncrementalGC(cx, JS::GCOptions::Shrink, JS::GCReason::API);
}

/// Reset the Math.random seed (used after wizer pre-initialization).
void sm_reset_math_random_seed(JSContext *cx) {
    js::ResetMathRandomSeed(cx);
}

} // extern "C"

// ── Math.random fix (use WASI randomness) ────────────────────────────────────

extern "C" {

/// Fix Math.random on a global to use WASI random instead of SM's PRNG.
/// This is important for deterministic wizer snapshots and server randomness.
bool sm_fix_math_random(JSContext *cx, JSObject *global,
                        bool (*random_fn)(JSContext*, unsigned, JS::Value*)) {
    JS::RootedObject rglobal(cx, global);
    JSAutoRealm ar(cx, rglobal);

    JS::RootedValue math_val(cx);
    if (!JS_GetProperty(cx, rglobal, "Math", &math_val)) return false;
    JS::RootedObject math(cx, &math_val.toObject());

    const JSFunctionSpec funs[] = {
        JS_FN("random", random_fn, 0, 0),
        JS_FS_END
    };
    return JS_DefineFunctions(cx, math, funs);
}

} // extern "C"

// ── JS CallArgs helpers (for Rust native functions) ──────────────────────────

extern "C" {

/// Get an argument value from a JS native function's vp.
JS::Value sm_call_args_get(uint32_t argc, JS::Value *vp, uint32_t index) {
    JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
    return args.get(index);
}

/// Set the return value of a JS native function to undefined.
void sm_call_args_rval_set_undefined(uint32_t argc, JS::Value *vp) {
    JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
    args.rval().setUndefined();
}

/// Set the return value of a JS native function.
void sm_call_args_rval_set(uint32_t argc, JS::Value *vp, JS::Value val) {
    JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
    args.rval().set(val);
}

/// Create a plain JS object.
JSObject *sm_new_plain_object(JSContext *cx) {
    return JS_NewPlainObject(cx);
}

/// Allocate a persistent root for a JSObject, returning a handle.
/// This is the public version of alloc_persistent_root.
int32_t sm_alloc_persistent_root(JSContext *cx, JSObject *obj) {
    if (!obj) return -1;
    return alloc_persistent_root(cx, obj);
}

} // extern "C"

// ── Property key enumeration ─────────────────────────────────────────────────

extern "C" {

bool sm_get_own_property_names(
    JSContext *cx, JSObject *obj,
    JSString ***out_strings, uint32_t *out_count
) {
    JS::RootedObject robj(cx, obj);
    JS::Rooted<JS::IdVector> ids(cx, JS::IdVector(cx));
    if (!JS_Enumerate(cx, robj, &ids)) {
        return false;
    }

    uint32_t count = ids.length();
    auto **strings = static_cast<JSString**>(malloc(count * sizeof(JSString*)));
    if (!strings) return false;

    for (uint32_t i = 0; i < count; i++) {
        JS::RootedValue idval(cx);
        if (!JS_IdToValue(cx, ids[i], &idval)) {
            free(strings);
            return false;
        }
        JS::RootedString str(cx, JS::ToString(cx, idval));
        if (!str) {
            free(strings);
            return false;
        }
        strings[i] = str;
    }

    *out_strings = strings;
    *out_count = count;
    return true;
}

} // extern "C"
