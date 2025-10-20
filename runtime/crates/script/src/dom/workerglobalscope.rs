/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use std::rc::Rc;
use js::rust::{HandleObject, MutableHandleObject};
use dom_struct::dom_struct;
use script_bindings::codegen::GenericBindings::WorkerGlobalScopeBinding::WorkerGlobalScopeMethods;
use script_bindings::reflector::DomObject;
use script_bindings::root::DomRoot;
use script_bindings::str::DOMString;
use servo_url::{MutableOrigin, ServoUrl};
use crate::base::id::PipelineId;
use crate::dom::bindings::cell::DomRefCell;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::globalscope::GlobalScope;
use crate::microtask::MicrotaskQueue;
use crate::Runtime;
use crate::script_runtime::JSContext;

#[dom_struct]
pub struct WorkerGlobalScope {
    global_scope: GlobalScope,
}

impl WorkerGlobalScope {

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_inherited(
        pipeline_id: PipelineId,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        microtask_queue: Rc<MicrotaskQueue>,
    ) -> Self {
        Self {
            global_scope: GlobalScope::new_inherited(
                pipeline_id,
                origin,
                creation_url,
                None,
                microtask_queue,
                // false,
            ),
        }
    }

    /// Get the JS context.
    pub(crate) fn get_cx() -> JSContext {
        GlobalScope::get_cx()
    }

    pub(crate) fn as_global_scope(&self) -> &GlobalScope {
        self.upcast::<GlobalScope>()
    }
}

impl WorkerGlobalScopeMethods<crate::DomTypeHolder> for WorkerGlobalScope {
    // https://html.spec.whatwg.org/multipage/#dom-workerglobalscope-self
    fn Self_(&self) -> DomRoot<WorkerGlobalScope> {
        DomRoot::from_ref(self)
    }
}

