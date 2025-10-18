/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use dom_struct::dom_struct;
use js::jsval::UndefinedValue;
use js::rust::ParentRuntime;
use servo_url::{MutableOrigin, ServoUrl};
use crate::base::id::PipelineId;
use crate::dom::bindings::cell::{DomRefCell, Ref};
use crate::dom::bindings::codegen::Bindings::StarlingGlobalScopeBinding;
use crate::dom::bindings::codegen::Bindings::StarlingGlobalScopeBinding::StarlingGlobalScopeMethods;
use crate::dom::bindings::error::{ErrorResult, Fallible, report_pending_exception};
use crate::dom::bindings::import::base::SafeJSContext;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::reflector::DomObject;
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::settings_stack::AutoEntryScript;
use crate::dom::bindings::str::DOMString;
use crate::dom::bindings::utils::define_all_exposed_interfaces;
use crate::dom::globalscope::GlobalScope;
use crate::realms::{InRealm, enter_realm};
use crate::script_runtime::{CanGc, IntroductionType, JSContext, JSContextHelper, Runtime};

// https://html.spec.whatwg.org/multipage/#the-workerglobalscope-common-interface
#[dom_struct]
pub struct StarlingGlobalScope {
    globalscope: GlobalScope,

    worker_name: DOMString,

    #[no_trace]
    worker_url: DomRefCell<ServoUrl>,
    #[ignore_malloc_size_of = "Defined in js"]
    runtime: DomRefCell<Option<Runtime>>,
}

impl StarlingGlobalScope {

    #[allow(unsafe_code, clippy::too_many_arguments)]
    pub fn new(
        pipeline_id: PipelineId,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
    ) -> DomRoot<Self> {
        let cx = runtime.cx();
        let scope = Box::new(Self::new_inherited(
            pipeline_id,
            origin,
            creation_url,
            worker_name,
            worker_url,
            runtime,
        ));
        unsafe {
            StarlingGlobalScopeBinding::Wrap::<crate::DomTypeHolder>(
                SafeJSContext::from_ptr(cx),
                scope,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_inherited(
        pipeline_id: PipelineId,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
    ) -> Self {
        Self {
            globalscope: GlobalScope::new_inherited(
                pipeline_id,
                origin,
                creation_url,
                None,
                runtime.microtask_queue.clone(),
                // false,
            ),
            worker_name,
            // worker_type,
            worker_url: DomRefCell::new(worker_url),
            runtime: DomRefCell::new(Some(runtime)),
            // location: Default::default(),
            // navigation_start: CrossProcessInstant::now(),
            // performance: Default::default(),
            // timer_scheduler: RefCell::default(),
        }
    }

    /// Clear various items when the worker event-loop shuts-down.
    pub(crate) fn clear_js_runtime(&self) {
        // Drop the runtime.
        let runtime = self.runtime.borrow_mut().take();
        drop(runtime);
    }

    pub(crate) fn runtime_handle(&self) -> ParentRuntime {
        self.runtime
            .borrow()
            .as_ref()
            .unwrap()
            .prepare_for_new_child()
    }

    #[allow(unsafe_code)]
    pub(crate) fn get_cx(&self) -> JSContext {
        unsafe { JSContext::from_ptr(self.runtime.borrow().as_ref().unwrap().cx()) }
    }

    pub(crate) fn get_url(&self) -> Ref<ServoUrl> {
        self.worker_url.borrow()
    }

    pub(crate) fn set_url(&self, url: ServoUrl) {
        *self.worker_url.borrow_mut() = url;
    }
}

impl StarlingGlobalScopeMethods<crate::DomTypeHolder> for StarlingGlobalScope {
    // https://html.spec.whatwg.org/multipage/#dom-workerglobalscope-self
    fn Self_(&self) -> DomRoot<StarlingGlobalScope> {
        DomRoot::from_ref(self)
    }
}

impl StarlingGlobalScope {

    /// <https://html.spec.whatwg.org/multipage/#run-a-worker>
    #[allow(unsafe_code, clippy::too_many_arguments)]
    pub fn run_worker_scope(
        pipeline_id: PipelineId,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_url: ServoUrl,
        worker_name: String,
    ) -> DomRoot<Self> {
        let runtime = Runtime::new();
        let global = Self::new(
            pipeline_id,
            origin,
            creation_url,
            DOMString::from_string(worker_name),
            worker_url,
            runtime,
        );
        let scope = global.upcast::<GlobalScope>();
        {
            let realm = enter_realm(scope);
            define_all_exposed_interfaces(
                scope,
                InRealm::entered(&realm),
                CanGc::note(),
            );
        }

        global
    }

    #[allow(unsafe_code)]
    pub fn execute_script(&self, source: DOMString, can_gc: CanGc) {
        let _aes = AutoEntryScript::new(self.upcast());
        let cx = self.runtime.borrow().as_ref().unwrap().cx();
        rooted!(in(cx) let mut rval = UndefinedValue());
        let mut options = self
            .runtime
            .borrow()
            .as_ref()
            .unwrap()
            .new_compile_options(self.worker_url.borrow().as_str(), 1);
        options.set_introduction_type(IntroductionType::WORKER);
        match self.runtime.borrow().as_ref().unwrap().evaluate_script(
            self.reflector().get_jsobject(),
            &source,
            rval.handle_mut(),
            options,
        ) {
            Ok(_) => (),
            Err(_) => {
                println!("evaluate_script failed");
                unsafe {
                    let ar = enter_realm(self);
                    report_pending_exception(
                        JSContext::from_ptr(cx),
                        true,
                        InRealm::Entered(&ar),
                        can_gc,
                    );
                }
            },
        }
        
        self.globalscope.perform_a_microtask_checkpoint(can_gc);
    }
}
