/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use std::rc::Rc;
use js::gc::{HandleValue, MutableHandleValue};
use js::rust::{HandleObject, MutableHandleObject};
use dom_struct::dom_struct;
use script_bindings::codegen::GenericBindings::MessagePortBinding::StructuredSerializeOptions;
use script_bindings::codegen::GenericBindings::RequestBinding::RequestInit;
use script_bindings::codegen::GenericBindings::VoidFunctionBinding::VoidFunction;
use script_bindings::codegen::GenericBindings::WindowBinding::WindowMethods;
use script_bindings::codegen::GenericUnionTypes::{RequestOrUSVString, TrustedScriptOrStringOrFunction};
use script_bindings::error::Fallible;
use script_bindings::interfaces::WindowHelpers;
use script_bindings::realms::InRealm;
use script_bindings::reflector::DomObject;
use script_bindings::script_runtime::CanGc;
use script_bindings::trace::RootedTraceableBox;
use crate::dom::bindings::codegen::DomTypeHolder::DomTypeHolder;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::globalscope::GlobalScope;
use crate::dom::promise::Promise;
use crate::script_runtime::JSContext;

#[dom_struct]
pub(crate) struct Window {
    global_scope: GlobalScope,
}

impl Window {
    /// Get the JS context.
    pub(crate) fn get_cx() -> JSContext {
        GlobalScope::get_cx()
    }

    pub(crate) fn as_global_scope(&self) -> &GlobalScope {
        self.upcast::<GlobalScope>()
    }
}

impl WindowHelpers for Window {
    fn create_named_properties_object(
        _cx: JSContext,
        _proto: HandleObject,
        _object: MutableHandleObject,
    ) {
        // Self::create_named_properties_object(cx, proto, object)
    }
}

impl WindowMethods<crate::DomTypeHolder> for Window {
    fn SetTimeout(&self, r#_cx: JSContext, r#_handler: TrustedScriptOrStringOrFunction<DomTypeHolder>, r#_timeout: i32, r#_arguments: Vec<HandleValue>, r#_can_gc: CanGc) -> Fallible<i32> {
        unreachable!("Window is never instantiated")
    }

    fn ClearTimeout(&self, r#_handle: i32) {
        unreachable!("Window is never instantiated")
    }

    fn SetInterval(&self, r#_cx: JSContext, r#_handler: TrustedScriptOrStringOrFunction<DomTypeHolder>, r#_timeout: i32, r#_arguments: Vec<HandleValue>, r#_can_gc: CanGc) -> Fallible<i32> {
        unreachable!("Window is never instantiated")
    }

    fn ClearInterval(&self, r#_handle: i32) {
        unreachable!("Window is never instantiated")
    }

    fn QueueMicrotask(&self, r#_callback: Rc<VoidFunction<DomTypeHolder>>) {
        unreachable!("Window is never instantiated")
    }

    fn StructuredClone(&self, r#cx: JSContext, r#value: HandleValue, r#options: RootedTraceableBox<StructuredSerializeOptions>, r#rval: MutableHandleValue) -> Fallible<()> {
        unreachable!("Window is never instantiated")
    }

    fn Fetch(&self, r#input: RequestOrUSVString<DomTypeHolder>, r#init: RootedTraceableBox<RequestInit<DomTypeHolder>>, r#_comp: InRealm, r#_can_gc: CanGc) -> Rc<Promise> {
        unreachable!("Window is never instantiated")
    }
}

