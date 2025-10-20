/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use js::rust::{HandleObject, MutableHandleObject};
use dom_struct::dom_struct;
use script_bindings::interfaces::WindowHelpers;
use script_bindings::reflector::DomObject;
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
        Self::create_named_properties_object(cx, proto, object)
    }
}

