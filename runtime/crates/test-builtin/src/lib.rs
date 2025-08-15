use lock::ThreadUnsafeOnceLock;
use std::ffi::{c_void, CStr};
use std::mem::ManuallyDrop;
// use spidermonkey_macros::js_class;
use spidermonkey_macros::error_type;
use spidermonkey_rs::conversions::{
    ConversionBehavior, ConversionResult, FromJSValConvertible, ToJSValConvertible,
};
use spidermonkey_rs::raw::JS::{CallArgs, GCContext, GCOptions, GCReason, HandleObject, Value};
use spidermonkey_rs::raw::{
    CallObjectTracer, CreateBuiltinClass, GetClass, JSErrorFormatString, JSNativeWrapper,
    JSPropertySpec_Name, JS_DefineFunction, JS_IsExceptionPending, JS_NewObjectForConstructor,
    NativeProperties, JS, JSCLASS_FOREGROUND_FINALIZE, JSPROP_ENUMERATE,
};
use spidermonkey_rs::raw::{
    JSClass, JSClassOps, JSContext, JSFunctionSpec, JSObject, JSPropertySpec, JSTracer,
    JS_GetReservedSlot, JS_SetReservedSlot, JSCLASS_IS_WRAPPED_NATIVE,
    JSCLASS_RESERVED_SLOTS_SHIFT,
};
use spidermonkey_rs::rust::{Handle, MutableHandle};

use crate::js_helpers::return_result;
use spidermonkey_rs::gc::Traceable;
use spidermonkey_rs::raw::js::GetFunctionNativeReserved;
use spidermonkey_rs::raw::JSExnType::JSEXN_TYPEERR;
use spidermonkey_rs::{jsval, root};
use starlingmonkey_rs::{throw_error, Engine};
use std::ptr;

mod lock;

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

mod adder {
    use super::*;
    use spidermonkey_rs::raw::JS::Heap;
    use std::cell::UnsafeCell;

    #[derive(Debug)]
    pub struct Adder {
        pub(super) object: Heap<*mut JSObject>,
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

        pub fn add(&self, cx: *mut JSContext, args: &CallArgs) -> bool {
            let a: Option<i32> = js_helpers::get_arg_typed(cx, args, 0);
            if a.is_none() {
                return throw_type_error(cx, c"Adder.add", c"argument 'a'", c"a Number");
            }
            let b: Option<i32> = js_helpers::get_arg_typed(cx, args, 1);
            if b.is_none() {
                return throw_type_error(cx, c"Adder.add", c"argument 'b'", c"a Number");
            }
            let result = a.unwrap() + b.unwrap();
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
            let res = native_this.add(cx, &args);
            if !res {
                assert!(
                    JS_IsExceptionPending(cx),
                    "Method call failed but no exception was set"
                );
            }
            // TODO: add check for whether rval was set.
            true
        }
    }

    static JS_CLASS: ThreadUnsafeOnceLock<JSClass> = ThreadUnsafeOnceLock::new();
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
        }

        unsafe fn js_class() -> &'static JSClass {
            JS_CLASS.get()
        }
        unsafe fn class_ops() -> &'static JSClassOps {
            CLASS_OPS.get()
        }

        unsafe extern "C" fn from_call_args(
            _cx: *mut JSContext,
            obj: HandleObject,
            args: *mut CallArgs,
        ) -> Value {
            let args = &*args;
            assert!(args.constructing_());
            let adder = ManuallyDrop::new(Box::new(Adder {
                object: Heap {
                    ptr: UnsafeCell::new(obj.get()),
                },
            }));
            jsval::PrivateValue(ptr::addr_of!(adder) as *const c_void)
            // adder.to_jsval(cx, MutableHandle::from_raw(args.rval()));
            // let mut adder = ManuallyDrop::new(Adder::new());
            // ptr::addr_of_mut!(adder) as *mut c_void
            // Alternatively, if you want to use a reserved slot:
            // c_void::try_from(adder.deref_mut() as *mut Adder)
            //     .expect("Failed to convert Adder to c_void")
            // adder.deref_mut() as *mut c_void
            // std::mem::forget(Adder::new()) as *const c_void
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

unsafe impl Traceable for adder::Adder {
    unsafe fn trace(&self, trc: *mut JSTracer) {
        // Trace the JSObject stored in the Adder instance
        println!("Tracing Adder instance: {:?}", self.object.get());
        CallObjectTracer(
            trc,
            self.object.get() as *mut _,
            c"Adder instance reflector".as_ptr(),
        );
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
        assert!(GetClass(this_obj) == T::js_class());
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
            assert!(!JS_IsExceptionPending(cx));
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
    unsafe fn class_ops() -> &'static JSClassOps;
    const CONSTRUCTOR_ARGC: u32 = 0;

    unsafe extern "C" fn finalize_hook(_gcx: *mut GCContext, obj: *mut JSObject) {
        let cls = GetClass(obj);
        assert!(!cls.is_null(), "Class can't be null");
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
        assert!(
            !ctor_obj.is_null(),
            "Constructor called without a valid 'this' object"
        );
        let cls = GetFunctionNativeReserved(ctor_obj, 0)
            .as_ref()
            .unwrap()
            .to_private() as *const JSClass;
        assert!(!cls.is_null(), "Class can't be null");

        root!(cx, let obj = JS_NewObjectForConstructor(cx, cls, &args));
        if obj.is_null() {
            return false;
        }

        let this = Self::from_call_args(cx, obj.handle().into(), &mut args);
        assert!(
            !this.is_null(),
            "Native constructor returned a null instance"
        );
        // Store the instance in the reserved slot
        JS_SetReservedSlot(obj.get(), 0, &this);
        return_result(cx, &args, obj.get());
        true
    }

    unsafe fn install_class(cx: *mut JSContext, global: HandleObject) -> bool {
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
            Self::js_class(), // TODO: this should be a different class for the prototype
            HandleObject::null(),
            global,
            true,
        );

        !proto.is_null()
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
