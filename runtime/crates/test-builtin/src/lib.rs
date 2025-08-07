use spidermonkey_rs::jsval::ObjectValue;
use spidermonkey_rs::raw::JSCLASS_FOREGROUND_FINALIZE;
use spidermonkey_rs::conversions::{ConversionBehavior, ConversionResult, FromJSValConvertible, ToJSValConvertible};
use spidermonkey_rs::raw::JS::{CallArgs, HandleObject, Value};
use spidermonkey_rs::raw::{JSClass, JSClassOps, JSContext, JSFunctionSpec, JSNativeWrapper, JSObject, JSPropertySpec, JSPropertySpec_Name, JS_NewObjectWithGivenProto, JSCLASS_RESERVED_SLOTS_MASK, JSCLASS_RESERVED_SLOTS_SHIFT, JSPROP_ENUMERATE};
use spidermonkey_rs::rust::wrapped::JS_InitClass;
use spidermonkey_rs::rust::{Handle, MutableHandle};
use spidermonkey_macros::impl_js_class;

use starlingmonkey_rs::Engine;
use std::ptr;

// Helper functions for common operations
mod js_helpers {
use spidermonkey_rs::jsval::Int32Value;
use spidermonkey_rs::raw::{JS_GetReservedSlot, JS_SetReservedSlot};
use super::*;

    pub unsafe fn return_error(cx: *mut JSContext, args: &CallArgs, message: &str) {
        let error_msg = format!("Error: {}", message);
        error_msg.to_jsval(cx, MutableHandle::from_raw(args.rval()));
    }

    pub unsafe fn return_result<T: ToJSValConvertible>(cx: *mut JSContext, args: &CallArgs, result: T) {
        result.to_jsval(cx, MutableHandle::from_raw(args.rval()));
    }

    pub unsafe fn get_arg_typed<T>(cx: *mut JSContext, args: &CallArgs, index: u32) -> Option<T>
    where
        T: FromJSValConvertible<Config = ConversionBehavior>,
    {
        if index >= args.argc_ {
            return None;
        }
        let val = args.get(index);
        match T::from_jsval(cx, Handle::from_raw(val), ConversionBehavior::Default) {
            Ok(ConversionResult::Success(value)) => Some(value),
            _ => None,
        }
    }

    // Reserved slot helpers
    pub unsafe fn get_reserved_slot_i32(obj: *mut JSObject, slot: u32) -> i32 {
        let mut val = Value::default();
        JS_GetReservedSlot(obj, slot, &mut val as *mut Value);
        if val.is_int32() {
            val.to_int32()
        } else {
            0
        }
    }

    pub unsafe fn set_reserved_slot_i32(obj: *mut JSObject, slot: u32, value: i32) {
        let val = Int32Value(value);
        JS_SetReservedSlot(obj, slot, &val as *const Value);
    }

    pub unsafe fn get_this_object(args: &CallArgs) -> Option<*mut JSObject> {
        let this_val = args.thisv();
        if this_val.is_object() {
            Some(this_val.to_object())
        } else {
            None
        }
    }
}

// Trait for defining JavaScript classes in Rust
pub trait JSClassTrait {
    const CLASS_NAME: &'static str;
    const JS_CLASS: &'static JSClass;
    const CLASS_OPS: JSClassOps = JSClassOps {
        addProperty: None,
        delProperty: None,
        enumerate: None,
        newEnumerate: None,
        resolve: None,
        mayResolve: None,
        finalize: None,
        call: None,
        construct: None,
        trace: None,
    };
    const CONSTRUCTOR_ARGC: u32 = 0;
    const RESERVED_SLOTS: u32 = 0;

    fn class_flags() -> u32 {
        let mut flags = 0u32;
        if Self::RESERVED_SLOTS > 0 {
            flags |= (Self::RESERVED_SLOTS & JSCLASS_RESERVED_SLOTS_MASK) << JSCLASS_RESERVED_SLOTS_SHIFT;
            // Add foreground finalize flag as required by SpiderMonkey for objects with reserved slots
            flags |= JSCLASS_FOREGROUND_FINALIZE;
        }
        flags
    }

    fn class_ops() -> JSClassOps {
        JSClassOps {
            addProperty: None,
            delProperty: None,
            enumerate: None,
            newEnumerate: None,
            resolve: None,
            mayResolve: None,
            finalize: None,
            call: None,
            construct: None,
            trace: None,
        }
    }

    fn methods() -> &'static [JSFunctionSpec] {
        static METHODS: [JSFunctionSpec; 4] = [
            JSFunctionSpec {
                name: JSPropertySpec_Name {
                    string_: b"getValue\0".as_ptr() as *const i8,
                },
                call: JSNativeWrapper {
                    op: Some(Counter::get_value),
                    info: ptr::null(),
                },
                nargs: 0,
                flags: JSPROP_ENUMERATE as u16,
                selfHostedName: ptr::null(),
            },
            JSFunctionSpec {
                name: JSPropertySpec_Name {
                    string_: b"setValue\0".as_ptr() as *const i8,
                },
                call: JSNativeWrapper {
                    op: Some(Counter::set_value),
                    info: ptr::null(),
                },
                nargs: 1,
                flags: JSPROP_ENUMERATE as u16,
                selfHostedName: ptr::null(),
            },
            JSFunctionSpec {
                name: JSPropertySpec_Name {
                    string_: b"increment\0".as_ptr() as *const i8,
                },
                call: JSNativeWrapper {
                    op: Some(Counter::increment),
                    info: ptr::null(),
                },
                nargs: 0,
                flags: JSPROP_ENUMERATE as u16,
                selfHostedName: ptr::null(),
            },
            JSFunctionSpec::ZERO,
        ];
        &METHODS
    }

    fn static_methods() -> &'static [JSFunctionSpec] {
        static STATIC_METHODS: [JSFunctionSpec; 3] = [
            JSFunctionSpec {
                name: JSPropertySpec_Name {
                    string_: b"create\0".as_ptr() as *const i8,
                },
                call: JSNativeWrapper {
                    op: Some(Counter::static_create),
                    info: ptr::null(),
                },
                nargs: 1,
                flags: JSPROP_ENUMERATE as u16,
                selfHostedName: ptr::null(),
            },
            JSFunctionSpec {
                name: JSPropertySpec_Name {
                    string_: b"version\0".as_ptr() as *const i8,
                },
                call: JSNativeWrapper {
                    op: Some(Counter::static_version),
                    info: ptr::null(),
                },
                nargs: 0,
                flags: JSPROP_ENUMERATE as u16,
                selfHostedName: ptr::null(),
            },
            JSFunctionSpec::ZERO,
        ];
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

    unsafe extern "C" fn constructor(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool;

    unsafe fn install_class(cx: *mut JSContext, global: HandleObject) -> bool {
        let proto = JS_InitClass(
            cx,
            global,
            Self::JS_CLASS,
            HandleObject::null(),
            Self::CLASS_NAME.as_ptr() as *const i8,
            Some(Self::constructor),
            Self::CONSTRUCTOR_ARGC,
            Self::properties().as_ptr(),
            Self::methods().as_ptr(),
            Self::static_properties().as_ptr(),
            Self::static_methods().as_ptr(),
        );

        !proto.is_null()
    }
}

// Counter class with simplified storage
struct Counter;

#[impl_js_class]
impl JSClassTrait for Counter {
    const CLASS_NAME: &'static str = "Counter";
    const CONSTRUCTOR_ARGC: u32 = 1;
    const RESERVED_SLOTS: u32 = 2; // Slot 0: value, Slot 1: name/id

    unsafe extern "C" fn constructor(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
        let args = CallArgs::from_vp(vp, argc);

        if !args.constructing_() {
            js_helpers::return_error(cx, &args, "Counter must be called with new");
            return false;
        }

        let initial_value: i32 = js_helpers::get_arg_typed(cx, &args, 0).unwrap_or(0);

        let obj = JS_NewObjectWithGivenProto(cx, Self::JS_CLASS, HandleObject::null());
        if obj.is_null() {
            js_helpers::return_error(cx, &args, "Failed to create Counter object");
            return false;
        }

        // Initialize reserved slots
        js_helpers::set_reserved_slot_i32(obj, 0, initial_value); // Value in slot 0
        js_helpers::set_reserved_slot_i32(obj, 1, 42); // Some ID/metadata in slot 1

        args.rval().set(ObjectValue(obj));
        true
    }

    fn methods() -> &'static [JSFunctionSpec] {
        static METHODS: [JSFunctionSpec; 1] = [
            JSFunctionSpec::ZERO,
        ];
        &METHODS
    }

    fn static_methods() -> &'static [JSFunctionSpec] {
        static STATIC_METHODS: [JSFunctionSpec; 1] = [
            JSFunctionSpec::ZERO,
        ];
        &STATIC_METHODS
    }
}

impl Counter {
    unsafe extern "C" fn get_value(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
        let args = CallArgs::from_vp(vp, argc);

        if let Some(obj) = js_helpers::get_this_object(&args) {
            let value = js_helpers::get_reserved_slot_i32(obj, 0);
            js_helpers::return_result(cx, &args, value);
        } else {
            js_helpers::return_error(cx, &args, "getValue called on non-Counter object");
        }
        true
    }

    unsafe extern "C" fn set_value(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
        let args = CallArgs::from_vp(vp, argc);

        if let Some(obj) = js_helpers::get_this_object(&args) {
            if let Some(new_value) = js_helpers::get_arg_typed::<i32>(cx, &args, 0) {
                js_helpers::set_reserved_slot_i32(obj, 0, new_value);
                js_helpers::return_result(cx, &args, new_value);
            } else {
                js_helpers::return_error(cx, &args, "setValue requires a number argument");
            }
        } else {
            js_helpers::return_error(cx, &args, "setValue called on non-Counter object");
        }
        true
    }

    unsafe extern "C" fn increment(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
        let args = CallArgs::from_vp(vp, argc);

        if let Some(obj) = js_helpers::get_this_object(&args) {
            let current_value = js_helpers::get_reserved_slot_i32(obj, 0);
            let new_value = current_value + 1;
            js_helpers::set_reserved_slot_i32(obj, 0, new_value);
            js_helpers::return_result(cx, &args, new_value);
        } else {
            js_helpers::return_error(cx, &args, "increment called on non-Counter object");
        }
        true
    }

    unsafe extern "C" fn static_create(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
        let args = CallArgs::from_vp(vp, argc);

        let initial_value: i32 = js_helpers::get_arg_typed(cx, &args, 0).unwrap_or(0);

        let obj = JS_NewObjectWithGivenProto(cx, Self::JS_CLASS, HandleObject::null());
        if obj.is_null() {
            js_helpers::return_error(cx, &args, "Failed to create Counter object");
            return false;
        }

        js_helpers::set_reserved_slot_i32(obj, 0, initial_value);
        js_helpers::set_reserved_slot_i32(obj, 1, 999); // Different ID for static creation

        args.rval().set(ObjectValue(obj));
        true
    }

    unsafe extern "C" fn static_version(cx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
        let args = CallArgs::from_vp(vp, argc);
        js_helpers::return_result(cx, &args, "Counter v1.0 - Rust Implementation".to_string());
        true
    }
}

// Enhanced framework installation
unsafe fn install_rust_framework(engine: &mut Engine) -> bool {

    // Test minimal JS_InitClass
    Counter::install_class(engine.cx(), engine.global());

    println!("Installed Rust JS Framework with JS_InitClass and reserved slots!");
    true
}

#[no_mangle]
pub unsafe extern "C" fn builtin_test_builtin_install(engine: &mut Engine) -> bool {
    install_rust_framework(engine);

    println!("test_builtin_install completed - JS_InitClass framework");
    true
}
