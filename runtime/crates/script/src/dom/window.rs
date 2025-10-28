/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use std::rc::Rc;
use js::gc::HandleValue;
use js::rust::{HandleObject, MutableHandleObject};
use dom_struct::dom_struct;
use script_bindings::codegen::GenericBindings::VoidFunctionBinding::VoidFunction;
use script_bindings::codegen::GenericBindings::WindowBinding::WindowMethods;
use script_bindings::codegen::GenericUnionTypes::TrustedScriptOrStringOrFunction;
use script_bindings::error::Fallible;
use script_bindings::interfaces::WindowHelpers;
use script_bindings::reflector::DomObject;
use script_bindings::script_runtime::CanGc;
use crate::dom::bindings::codegen::DomTypeHolder::DomTypeHolder;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::globalscope::GlobalScope;
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
        cx: JSContext,
        proto: HandleObject,
        object: MutableHandleObject,
    ) {
        // Self::create_named_properties_object(cx, proto, object)
    }
}

impl WindowMethods<crate::DomTypeHolder> for Window {
    fn SetTimeout(&self, r#cx: JSContext, r#handler: TrustedScriptOrStringOrFunction<DomTypeHolder>, r#timeout: i32, r#arguments: Vec<HandleValue>, r#_can_gc: CanGc) -> Fallible<i32> {
        unreachable!("Window is never instantiated")
    }

    fn ClearTimeout(&self, r#handle: i32) {
        unreachable!("Window is never instantiated")
    }

    fn SetInterval(&self, r#cx: JSContext, r#handler: TrustedScriptOrStringOrFunction<DomTypeHolder>, r#timeout: i32, r#arguments: Vec<HandleValue>, r#_can_gc: CanGc) -> Fallible<i32> {
        unreachable!("Window is never instantiated")
    }

    fn ClearInterval(&self, r#handle: i32) {
        unreachable!("Window is never instantiated")
    }

    fn QueueMicrotask(&self, r#callback: Rc<VoidFunction<DomTypeHolder>>) {
        unreachable!("Window is never instantiated")
    }
}

