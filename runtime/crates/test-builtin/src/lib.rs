extern crate js;
#[macro_use]
extern crate jstraceable_derive;
extern crate log;
use dom_struct::dom_struct;
use spidermonkey_macros::error_type;
use spidermonkey_rs::conversions::{
    ConversionBehavior, ConversionResult, FromJSValConvertible, ToJSValConvertible,
};
use spidermonkey_rs::jsapi::JS_NewObjectForConstructor;
use spidermonkey_rs::raw::{
    CreateBuiltinClass, GetClass, JSClass, JSClassOps, JSContext, JSErrorFormatString,
    JSFunctionSpec, JSNativeWrapper, JSObject, JSPropertySpec, JSPropertySpec_Name, JSTracer,
    JS_DefineFunction, JS_GetReservedSlot, JS_IsExceptionPending, JS_SetReservedSlot,
    NativeProperties, JS, JSCLASS_FOREGROUND_FINALIZE, JSCLASS_IS_WRAPPED_NATIVE,
    JSCLASS_RESERVED_SLOTS_SHIFT, JSPROP_ENUMERATE,
};
use spidermonkey_rs::rooted;
use spidermonkey_rs::rust::{Handle, MutableHandle};
use std::ffi::{c_void, CStr};
use std::mem::ManuallyDrop;
use std::ptr;
use JS::{CallArgs, GCContext, GCOptions, GCReason, HandleObject, Value};

use crate::js_helpers::return_result;
pub(crate) use js::gc::Traceable as JSTraceable;
use script_bindings::inheritance::HasParent;
use spidermonkey_rs::gc::Traceable;
use spidermonkey_rs::jsval;
use spidermonkey_rs::raw::js::GetFunctionNativeReserved;
use spidermonkey_rs::raw::JSExnType::JSEXN_TYPEERR;
use starlingmonkey_rs::{
    throw_error, Engine, Engine_get_builtin_proto, Engine_register_builtin_proto,
    Engine_reserve_builtin_proto_id,
};

error_type!(
    WrongReceiver,
    JSEXN_TYPEERR,
    "Method '{0}' called on receiver that's not an instance of {1}"
);
error_type!(
    CtorCalledWithoutNew,
    JSEXN_TYPEERR,
    "{0} constructor called without 'new'"
);
error_type!(
    NoCtorBuiltin,
    JSEXN_TYPEERR,
    "{0} builtin can't be instantiated directly"
);
error_type!(TypeError, JSEXN_TYPEERR, "{0}: {1} must {2}");

#[repr(transparent)]
pub struct GCRef<T: Traceable> {
    ptr: ptr::NonNull<T>,
}

use script_bindings::lock::ThreadUnsafeOnceLock;
use script_bindings::reflector::DomObject;
use script_bindings::reflector::MutDomObject;
use script_bindings::reflector::Reflector;
mod adder {
    use super::*;

    #[dom_struct]
    pub struct Adder {
        object: Reflector,
    }

    impl Adder {
        pub fn static_add(a: i32, b: i32) -> i32 {
            a + b
        }
        unsafe extern "C" fn static_add_method_wrapper(
            cx: *mut JSContext,
            argc: u32,
            vp: *mut Value,
        ) -> bool {
            let args = CallArgs::from_vp(vp, argc);
            let a: Option<i32> = js_helpers::get_arg_typed(cx, &args, 0);
            if a.is_none() {
                return throw_type_error(cx, c"Adder.add", c"argument 'a'", c"a Number");
            }
            let b: Option<i32> = js_helpers::get_arg_typed(cx, &args, 1);
            if b.is_none() {
                return throw_type_error(cx, c"Adder.add", c"argument 'b'", c"a Number");
            }
            let result = Adder::static_add(a.unwrap(), b.unwrap());
            js_helpers::return_result(cx, &args, result);
            true
        }

        pub fn add(&self, a: i32, b: i32) -> i32 {
            a + b
        }

        pub(super) fn add_method(&self, cx: *mut JSContext, args: &CallArgs) -> bool {
            let a: Option<i32> = js_helpers::get_arg_typed(cx, args, 0);
            if a.is_none() {
                return throw_type_error(cx, c"Adder.add", c"argument 'a'", c"a Number");
            }
            let b: Option<i32> = js_helpers::get_arg_typed(cx, args, 1);
            if b.is_none() {
                return throw_type_error(cx, c"Adder.add", c"argument 'b'", c"a Number");
            }
            let result = self.add(a.unwrap(), b.unwrap());
            js_helpers::return_result(cx, args, result)
        }

        unsafe extern "C" fn add_method_wrapper(
            cx: *mut JSContext,
            argc: u32,
            vp: *mut Value,
        ) -> bool {
            let args = CallArgs::from_vp(vp, argc);

            let native_this: &Adder = if let Ok(value) = js_helpers::native_receiver(cx, &args) {
                value
            } else {
                return false;
            };
            let res = native_this.add_method(cx, &args);
            if !res {
                debug_assert!(
                    JS_IsExceptionPending(cx),
                    "Method call failed but no exception was set"
                );
            }
            // TODO: add check for whether rval was set.
            true
        }
    }

    static JS_CLASS: ThreadUnsafeOnceLock<JSClass> = ThreadUnsafeOnceLock::new();
    static PROTO_ID: ThreadUnsafeOnceLock<usize> = ThreadUnsafeOnceLock::new();
    static JS_PROTO_CLASS: ThreadUnsafeOnceLock<JSClass> = ThreadUnsafeOnceLock::new();
    static CLASS_OPS: ThreadUnsafeOnceLock<JSClassOps> = ThreadUnsafeOnceLock::new();

    impl JSBuiltinClass for Adder {
        unsafe fn init_js_class() {
            CLASS_OPS.set(JSClassOps {
                addProperty: None,
                delProperty: None,
                enumerate: None,
                newEnumerate: None,
                resolve: None,
                mayResolve: None,
                finalize: Some(Self::finalize_hook),
                call: None,
                construct: None,
                trace: Some(Self::trace_hook),
            });
            JS_CLASS.set(JSClass {
                name: c"Adder".as_ptr() as *const i8,
                flags: JSCLASS_IS_WRAPPED_NATIVE
                    | 1 << JSCLASS_RESERVED_SLOTS_SHIFT
                    | JSCLASS_FOREGROUND_FINALIZE,
                cOps: CLASS_OPS.get(),
                spec: ptr::null(),
                ext: ptr::null(),
                oOps: ptr::null(),
            });
            JS_PROTO_CLASS.set(JSClass {
                name: c"Adder_proto".as_ptr() as *const i8,
                flags: 0,
                cOps: ptr::null(),
                spec: ptr::null(),
                ext: ptr::null(),
                oOps: ptr::null(),
            });
        }

        unsafe fn js_class() -> &'static JSClass {
            JS_CLASS.get()
        }
        unsafe fn js_proto_class() -> &'static JSClass {
            JS_PROTO_CLASS.get()
        }
        unsafe fn class_ops() -> &'static JSClassOps {
            CLASS_OPS.get()
        }
        unsafe fn proto_id() -> usize {
            *PROTO_ID.get()
        }
        unsafe fn set_proto_id(id: usize) {
            PROTO_ID.set(id);
        }

        unsafe extern "C" fn from_call_args(
            _cx: *mut JSContext,
            obj: HandleObject,
            args: *mut CallArgs,
        ) -> Value {
            let args = &*args;
            debug_assert!(args.constructing_());
            let adder = ManuallyDrop::new(Box::new(Adder {
                object: Reflector::new(),
            }));
            adder.object.init_reflector(obj.get());
            jsval::PrivateValue(ptr::addr_of!(adder) as *const c_void)
        }

        fn methods() -> &'static [JSFunctionSpec] {
            static METHODS: [JSFunctionSpec; 2] = [
                JSFunctionSpec {
                    name: JSPropertySpec_Name {
                        string_: c"add".as_ptr(),
                    },
                    call: JSNativeWrapper {
                        op: Some(Adder::add_method_wrapper),
                        info: ptr::null(),
                    },
                    nargs: 2,
                    flags: JSPROP_ENUMERATE as u16,
                    selfHostedName: ptr::null(),
                },
                JSFunctionSpec::ZERO,
            ];
            &METHODS
        }

        fn static_methods() -> &'static [JSFunctionSpec] {
            static METHODS: [JSFunctionSpec; 2] = [
                JSFunctionSpec {
                    name: JSPropertySpec_Name {
                        string_: c"static_add".as_ptr(),
                    },
                    call: JSNativeWrapper {
                        op: Some(Adder::static_add_method_wrapper),
                        info: ptr::null(),
                    },
                    nargs: 2,
                    flags: JSPROP_ENUMERATE as u16,
                    selfHostedName: ptr::null(),
                },
                JSFunctionSpec::ZERO,
            ];
            &METHODS
        }
    }
}

mod mather {
    use super::*;

    #[dom_struct]
    pub struct Mather {
        object: Reflector,
    }

    impl Mather {
        pub fn add(&self, a: i32, b: i32) -> i32 {
            a + b
        }

        pub(super) fn add_method(&self, cx: *mut JSContext, args: &CallArgs) -> bool {
            let a: Option<i32> = js_helpers::get_arg_typed(cx, args, 0);
            if a.is_none() {
                return throw_type_error(cx, c"Mather.add", c"argument 'a'", c"a Number");
            }
            let b: Option<i32> = js_helpers::get_arg_typed(cx, args, 1);
            if b.is_none() {
                return throw_type_error(cx, c"Mather.add", c"argument 'b'", c"a Number");
            }
            let result = self.add(a.unwrap(), b.unwrap());
            js_helpers::return_result(cx, args, result)
        }

        unsafe extern "C" fn add_method_wrapper(
            cx: *mut JSContext,
            argc: u32,
            vp: *mut Value,
        ) -> bool {
            let args = CallArgs::from_vp(vp, argc);

            let native_this: &Mather = if let Ok(value) = js_helpers::native_receiver(cx, &args) {
                value
            } else {
                return false;
            };
            let res = native_this.add_method(cx, &args);
            if !res {
                debug_assert!(
                    JS_IsExceptionPending(cx),
                    "Method call failed but no exception was set"
                );
            }
            // TODO: add check for whether rval was set.
            true
        }
    }

    static JS_CLASS: ThreadUnsafeOnceLock<JSClass> = ThreadUnsafeOnceLock::new();
    static PROTO_ID: ThreadUnsafeOnceLock<usize> = ThreadUnsafeOnceLock::new();
    static JS_PROTO_CLASS: ThreadUnsafeOnceLock<JSClass> = ThreadUnsafeOnceLock::new();
    static CLASS_OPS: ThreadUnsafeOnceLock<JSClassOps> = ThreadUnsafeOnceLock::new();

    impl JSBuiltinClass for Mather {
        unsafe fn init_js_class() {
            CLASS_OPS.set(JSClassOps {
                addProperty: None,
                delProperty: None,
                enumerate: None,
                newEnumerate: None,
                resolve: None,
                mayResolve: None,
                finalize: Some(Self::finalize_hook),
                call: None,
                construct: None,
                trace: Some(Self::trace_hook),
            });
            JS_CLASS.set(JSClass {
                name: c"Mather".as_ptr() as *const i8,
                flags: JSCLASS_IS_WRAPPED_NATIVE
                    | 1 << JSCLASS_RESERVED_SLOTS_SHIFT
                    | JSCLASS_FOREGROUND_FINALIZE,
                cOps: CLASS_OPS.get(),
                spec: ptr::null(),
                ext: ptr::null(),
                oOps: ptr::null(),
            });
            JS_PROTO_CLASS.set(JSClass {
                name: c"Mather_proto".as_ptr() as *const i8,
                flags: 0,
                cOps: ptr::null(),
                spec: ptr::null(),
                ext: ptr::null(),
                oOps: ptr::null(),
            });
        }

        unsafe fn js_class() -> &'static JSClass {
            JS_CLASS.get()
        }
        unsafe fn js_proto_class() -> &'static JSClass {
            JS_PROTO_CLASS.get()
        }
        unsafe fn class_ops() -> &'static JSClassOps {
            CLASS_OPS.get()
        }
        unsafe fn proto_id() -> usize {
            *PROTO_ID.get()
        }
        unsafe fn set_proto_id(id: usize) {
            PROTO_ID.set(id);
        }

        unsafe extern "C" fn from_call_args(
            _cx: *mut JSContext,
            obj: HandleObject,
            args: *mut CallArgs,
        ) -> Value {
            let args = &*args;
            debug_assert!(args.constructing_());
            let mather = ManuallyDrop::new(Box::new(Mather {
                object: Reflector::new(),
            }));
            mather.object.init_reflector(obj.get());
            jsval::PrivateValue(ptr::addr_of!(mather) as *const c_void)
        }

        fn methods() -> &'static [JSFunctionSpec] {
            static METHODS: [JSFunctionSpec; 2] = [
                JSFunctionSpec {
                    name: JSPropertySpec_Name {
                        string_: c"add".as_ptr(),
                    },
                    call: JSNativeWrapper {
                        op: Some(Mather::add_method_wrapper),
                        info: ptr::null(),
                    },
                    nargs: 2,
                    flags: JSPROP_ENUMERATE as u16,
                    selfHostedName: ptr::null(),
                },
                JSFunctionSpec::ZERO,
            ];
            &METHODS
        }

        fn static_methods() -> &'static [JSFunctionSpec] {
            static METHODS: [JSFunctionSpec; 1] = [JSFunctionSpec::ZERO];
            &METHODS
        }
    }
}

// Helper functions for common operations
mod js_helpers {
    use super::*;

    pub unsafe fn native_receiver<'a, T: JSBuiltinClass>(
        cx: *mut JSContext,
        args: &'a CallArgs,
    ) -> Result<&'a T, bool> {
        if !args.thisv().is_object() || GetClass(args.thisv().to_object()) != T::js_class() {
            return Err(throw_wrong_receiver(cx, c"add", c"T"));
        }
        let this_obj = args.thisv().to_object();

        this_object(this_obj)
    }

    unsafe fn this_object<'a, T: JSBuiltinClass>(this_obj: *mut JSObject) -> Result<&'a T, bool> {
        debug_assert!(GetClass(this_obj) == T::js_class());
        let mut this_val = Value::default();
        JS_GetReservedSlot(this_obj, 0, &mut this_val as *mut Value);
        Ok(&*(this_val.to_private() as *const T))
    }

    pub fn return_result<T: ToJSValConvertible>(
        cx: *mut JSContext,
        args: &CallArgs,
        result: T,
    ) -> bool {
        unsafe {
            debug_assert!(!JS_IsExceptionPending(cx));
            result.to_jsval(cx, MutableHandle::from_raw(args.rval()));
            !JS_IsExceptionPending(cx)
        }
    }

    pub fn get_arg_typed<T>(cx: *mut JSContext, args: &CallArgs, index: u32) -> Option<T>
    where
        T: FromJSValConvertible<Config = ConversionBehavior>,
    {
        if index >= args.argc_ {
            return None;
        }
        let val = args.get(index);
        unsafe {
            match T::from_jsval(cx, Handle::from_raw(val), ConversionBehavior::Default) {
                Ok(ConversionResult::Success(value)) => Some(value),
                _ => None,
            }
        }
    }
}

pub trait JSBuiltinClass {
    unsafe fn init_js_class();
    unsafe fn js_class() -> &'static JSClass;
    unsafe fn js_proto_class() -> &'static JSClass;
    unsafe fn class_ops() -> &'static JSClassOps;
    fn proto(global: HandleObject) -> HandleObject {
        unsafe { Engine_get_builtin_proto(*global, Self::proto_id()) }
    }

    unsafe fn proto_id() -> usize;
    unsafe fn set_proto_id(id: usize);

    const CONSTRUCTOR_ARGC: u32 = 0;

    unsafe extern "C" fn finalize_hook(_gcx: *mut GCContext, obj: *mut JSObject) {
        let cls = GetClass(obj);
        debug_assert!(!cls.is_null(), "Class can't be null");
        println!(
            "Finalizing class instance: {:?}",
            CStr::from_ptr((*cls).name)
        );
    }

    unsafe extern "C" fn trace_hook(_trc: *mut JSTracer, _obj: *mut JSObject) {
        println!("Tracing class instance");
        // No-op for now, can be overridden if needed
    }

    fn methods() -> &'static [JSFunctionSpec] {
        static METHODS: [JSFunctionSpec; 1] = [JSFunctionSpec::ZERO];
        &METHODS
    }

    fn static_methods() -> &'static [JSFunctionSpec] {
        static STATIC_METHODS: [JSFunctionSpec; 1] = [JSFunctionSpec::ZERO];
        &STATIC_METHODS
    }

    fn properties() -> &'static [JSPropertySpec] {
        static EMPTY: [JSPropertySpec; 1] = [JSPropertySpec::ZERO];
        &EMPTY
    }

    fn static_properties() -> &'static [JSPropertySpec] {
        static EMPTY: [JSPropertySpec; 1] = [JSPropertySpec::ZERO];
        &EMPTY
    }

    unsafe extern "C" fn from_call_args(
        cx: *mut JSContext,
        obj: HandleObject,
        args: *mut CallArgs,
    ) -> Value;

    unsafe extern "C" fn js_constructor(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
        let mut args = CallArgs::from_vp(vp, argc);
        if !args.constructing_() {
            return throw_ctor_called_without_new(cx, c"Adder");
        }
        let ctor_obj = args.callee();
        debug_assert!(
            !ctor_obj.is_null(),
            "Constructor called without a valid 'this' object"
        );
        let cls = GetFunctionNativeReserved(ctor_obj, 0)
            .as_ref()
            .unwrap()
            .to_private() as *const JSClass;
        debug_assert!(!cls.is_null(), "Class can't be null");

        rooted!(in(cx) let obj = JS_NewObjectForConstructor(cx, cls, &args));
        if obj.is_null() {
            return false;
        }

        let this = Self::from_call_args(cx, obj.handle().into(), &mut args);
        debug_assert!(
            !this.is_null(),
            "Native constructor returned a null instance"
        );
        // Store the instance in the reserved slot
        JS_SetReservedSlot(obj.get(), 0, &this);
        return_result(cx, &args, obj.get());
        true
    }

    unsafe fn install_class(cx: *mut JSContext, global: HandleObject) -> bool {
        Self::set_proto_id(Engine_reserve_builtin_proto_id());
        let properties = NativeProperties {
            methods: Self::methods().as_ptr(),
            properties: Self::properties().as_ptr(),
            constants: ptr::null(),
        };

        let ctor_properties = NativeProperties {
            methods: Self::static_methods().as_ptr(),
            properties: Self::static_properties().as_ptr(),
            constants: ptr::null(),
        };
        Self::init_js_class();

        let proto = CreateBuiltinClass(
            cx,
            Some(Self::js_constructor),
            Self::CONSTRUCTOR_ARGC,
            Self::js_class(),
            &properties as *const _,
            &ctor_properties as *const _,
            Self::js_proto_class(),
            HandleObject::null(),
            global,
            true,
        );

        if proto.is_null() {
            return false;
        }

        Engine_register_builtin_proto(*global, proto, Self::proto_id());
        true
    }
}

unsafe extern "C" fn gc(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
    JS::NonIncrementalGC(cx, GCOptions::Normal, GCReason::API);
    let args = CallArgs::from_vp(vp, argc);
    args.rval().set(jsval::UndefinedValue());
    true
}

#[no_mangle]
pub unsafe extern "C" fn builtin_test_builtin_install(engine: &mut Engine) -> bool {
    adder::Adder::install_class(engine.cx(), engine.global());
    mather::Mather::install_class(engine.cx(), engine.global());
    JS_DefineFunction(
        engine.cx(),
        engine.global(),
        c"gc".as_ptr(),
        Some(gc),
        0,
        JSPROP_ENUMERATE.into(),
    );

    true
}
