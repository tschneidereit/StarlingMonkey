/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::{RefCell, RefMut};
use std::default::Default;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

// use base::cross_process_instant::CrossProcessInstant;
// use base::id::{PipelineId, PipelineNamespace};
// use constellation_traits::WorkerGlobalScopeInit;
// use content_security_policy::CspList;
// use crossbeam_channel::Receiver;
// use devtools_traits::{DevtoolScriptControlMsg, WorkerId};
use dom_struct::dom_struct;
// use ipc_channel::ipc::IpcSender;
use js::jsval::UndefinedValue;
use js::panic::maybe_resume_unwind;
use js::rust::{HandleValue, MutableHandleValue, ParentRuntime};
// use net_traits::policy_container::PolicyContainer;
// use net_traits::request::{
//     CredentialsMode, Destination, InsecureRequestsPolicy, ParserMetadata,
//     RequestBuilder as NetRequestInit,
// };
// use net_traits::{IpcSend, ReferrerPolicy};
// use profile_traits::mem::{ProcessReports, perform_memory_report};
use servo_url::{MutableOrigin, ServoUrl};
// use timers::TimerScheduler;
// use uuid::Uuid;

use crate::dom::bindings::cell::{DomRefCell, Ref};
// use crate::dom::bindings::codegen::Bindings::ReportingObserverBinding::Report;
// use crate::dom::bindings::codegen::Bindings::RequestBinding::RequestInit;
// use crate::dom::bindings::codegen::Bindings::VoidFunctionBinding::VoidFunction;
// use crate::dom::bindings::codegen::Bindings::WorkerBinding::WorkerType;
use crate::dom::bindings::codegen::Bindings::WorkerGlobalScopeBinding::WorkerGlobalScopeMethods;
// use crate::dom::bindings::codegen::UnionTypes::{
//     RequestOrUSVString, StringOrFunction, TrustedScriptURLOrUSVString,
// };
use crate::dom::bindings::error::{Error, ErrorResult, Fallible, report_pending_exception};
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::refcounted::Trusted;
use crate::dom::bindings::reflector::DomObject;
use crate::dom::bindings::root::{DomRoot, MutNullableDom};
use crate::dom::bindings::settings_stack::AutoEntryScript;
use crate::dom::bindings::str::{DOMString, USVString};
use crate::dom::bindings::trace::RootedTraceableBox;
// use crate::dom::crypto::Crypto;
// use crate::dom::csp::{GlobalCspReporting, Violation};
use crate::dom::dedicatedworkerglobalscope::DedicatedWorkerGlobalScope;
use crate::dom::globalscope::GlobalScope;
// use crate::dom::idbfactory::IDBFactory;
// use crate::dom::performance::Performance;
// use crate::dom::promise::Promise;
// use crate::dom::reportingendpoint::{ReportingEndpoint, SendReportsToEndpoints};
// use crate::dom::reportingobserver::ReportingObserver;
// use crate::dom::trustedscripturl::TrustedScriptURL;
// use crate::dom::trustedtypepolicyfactory::TrustedTypePolicyFactory;
// use crate::dom::types::ImageBitmap;
#[cfg(feature = "webgpu")]
use crate::dom::webgpu::identityhub::IdentityHub;
// use crate::dom::window::{base64_atob, base64_btoa};
// use crate::dom::workerlocation::WorkerLocation;
// use crate::dom::workernavigator::WorkerNavigator;
// use crate::fetch::{CspViolationsProcessor, Fetch, load_whole_resource};
// use crate::messaging::{CommonScriptMsg, ScriptEventLoopReceiver, ScriptEventLoopSender};
use crate::realms::{InRealm, enter_realm};
use crate::script_runtime::{CanGc, IntroductionType, JSContext, JSContextHelper, Runtime};
use crate::task::TaskCanceller;
// use crate::timers::{IsInterval, TimerCallback};

// pub(crate) fn prepare_workerscope_init(
//     global: &GlobalScope,
//     // devtools_sender: Option<IpcSender<DevtoolScriptControlMsg>>,
//     // worker_id: Option<WorkerId>,
// ) -> WorkerGlobalScopeInit {
//     let init = WorkerGlobalScopeInit {
//         resource_threads: global.resource_threads().clone(),
//         mem_profiler_chan: global.mem_profiler_chan().clone(),
//         to_devtools_sender: global.devtools_chan().cloned(),
//         time_profiler_chan: global.time_profiler_chan().clone(),
//         from_devtools_sender: devtools_sender,
//         script_to_constellation_chan: global.script_to_constellation_chan().clone(),
//         worker_id: worker_id.unwrap_or_else(|| WorkerId(Uuid::new_v4())),
//         pipeline_id: global.pipeline_id(),
//         origin: global.origin().immutable().clone(),
//         creation_url: global.creation_url().clone(),
//         inherited_secure_context: Some(global.is_secure_context()),
//     };
//
//     init
// }

// https://html.spec.whatwg.org/multipage/#the-workerglobalscope-common-interface
#[dom_struct]
pub struct WorkerGlobalScope {
    globalscope: GlobalScope,

    worker_name: DOMString,

    #[no_trace]
    worker_url: DomRefCell<ServoUrl>,
    #[ignore_malloc_size_of = "Defined in js"]
    runtime: DomRefCell<Option<Runtime>>,
    // location: MutNullableDom<WorkerLocation>,

    // #[no_trace]
    // navigation_start: CrossProcessInstant,
    // performance: MutNullableDom<Performance>,
    //
    // /// A [`TimerScheduler`] used to schedule timers for this [`WorkerGlobalScope`].
    // /// Timers are handled in the service worker event loop.
    // #[no_trace]
    // timer_scheduler: RefCell<TimerScheduler>,
}

impl WorkerGlobalScope {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_inherited(
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
    ) -> Self {
        Self {
            globalscope: GlobalScope::new_inherited(
                origin,
                creation_url,
                None,
                // runtime.microtask_queue.clone(),
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
        // self.upcast::<GlobalScope>()
        //     .remove_web_messaging_and_dedicated_workers_infra();

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

    // /// Get a mutable reference to the [`TimerScheduler`] for this [`ServiceWorkerGlobalScope`].
    // pub(crate) fn timer_scheduler(&self) -> RefMut<TimerScheduler> {
    //     self.timer_scheduler.borrow_mut()
    // }
}

impl WorkerGlobalScopeMethods<crate::DomTypeHolder> for WorkerGlobalScope {
    // https://html.spec.whatwg.org/multipage/#dom-workerglobalscope-self
    fn Self_(&self) -> DomRoot<WorkerGlobalScope> {
        DomRoot::from_ref(self)
    }

    // // https://w3c.github.io/IndexedDB/#factory-interface
    // fn IndexedDB(&self) -> DomRoot<IDBFactory> {
    //     self.indexeddb.or_init(|| {
    //         let global_scope = self.upcast::<GlobalScope>();
    //         IDBFactory::new(global_scope, CanGc::note())
    //     })
    // }
    //
    // // https://html.spec.whatwg.org/multipage/#dom-workerglobalscope-location
    // fn Location(&self) -> DomRoot<WorkerLocation> {
    //     self.location
    //         .or_init(|| WorkerLocation::new(self, self.worker_url.borrow().clone(), CanGc::note()))
    // }
    //
    // // https://html.spec.whatwg.org/multipage/#handler-workerglobalscope-onerror
    // error_event_handler!(error, GetOnerror, SetOnerror);
    //
    // // https://html.spec.whatwg.org/multipage/#dom-worker-navigator
    // fn Navigator(&self) -> DomRoot<WorkerNavigator> {
    //     self.navigator
    //         .or_init(|| WorkerNavigator::new(self, CanGc::note()))
    // }

    // // https://html.spec.whatwg.org/multipage/#dfn-Crypto
    // fn Crypto(&self) -> DomRoot<Crypto> {
    //     self.upcast::<GlobalScope>().crypto(CanGc::note())
    // }
    //
    // // https://html.spec.whatwg.org/multipage/#dom-windowbase64-btoa
    // fn Btoa(&self, btoa: DOMString) -> Fallible<DOMString> {
    //     base64_btoa(btoa)
    // }
    //
    // // https://html.spec.whatwg.org/multipage/#dom-windowbase64-atob
    // fn Atob(&self, atob: DOMString) -> Fallible<DOMString> {
    //     base64_atob(atob)
    // }

    // // https://html.spec.whatwg.org/multipage/#dom-windowtimers-settimeout
    // fn SetTimeout(
    //     &self,
    //     _cx: JSContext,
    //     callback: StringOrFunction,
    //     timeout: i32,
    //     args: Vec<HandleValue>,
    // ) -> i32 {
    //     let callback = match callback {
    //         StringOrFunction::String(i) => TimerCallback::StringTimerCallback(i),
    //         StringOrFunction::Function(i) => TimerCallback::FunctionTimerCallback(i),
    //     };
    //     self.upcast::<GlobalScope>().set_timeout_or_interval(
    //         callback,
    //         args,
    //         Duration::from_millis(timeout.max(0) as u64),
    //         IsInterval::NonInterval,
    //     )
    // }
    //
    // // https://html.spec.whatwg.org/multipage/#dom-windowtimers-cleartimeout
    // fn ClearTimeout(&self, handle: i32) {
    //     self.upcast::<GlobalScope>()
    //         .clear_timeout_or_interval(handle);
    // }
    //
    // // https://html.spec.whatwg.org/multipage/#dom-windowtimers-setinterval
    // fn SetInterval(
    //     &self,
    //     _cx: JSContext,
    //     callback: StringOrFunction,
    //     timeout: i32,
    //     args: Vec<HandleValue>,
    // ) -> i32 {
    //     let callback = match callback {
    //         StringOrFunction::String(i) => TimerCallback::StringTimerCallback(i),
    //         StringOrFunction::Function(i) => TimerCallback::FunctionTimerCallback(i),
    //     };
    //     self.upcast::<GlobalScope>().set_timeout_or_interval(
    //         callback,
    //         args,
    //         Duration::from_millis(timeout.max(0) as u64),
    //         IsInterval::Interval,
    //     )
    // }
    //
    // // https://html.spec.whatwg.org/multipage/#dom-windowtimers-clearinterval
    // fn ClearInterval(&self, handle: i32) {
    //     self.ClearTimeout(handle);
    // }

    // // https://html.spec.whatwg.org/multipage/#dom-queuemicrotask
    // fn QueueMicrotask(&self, callback: Rc<VoidFunction>) {
    //     self.upcast::<GlobalScope>()
    //         .queue_function_as_microtask(callback);
    // }
    //
    // #[cfg_attr(crown, allow(crown::unrooted_must_root))]
    // // https://fetch.spec.whatwg.org/#fetch-method
    // fn Fetch(
    //     &self,
    //     input: RequestOrUSVString,
    //     init: RootedTraceableBox<RequestInit>,
    //     comp: InRealm,
    //     can_gc: CanGc,
    // ) -> Rc<Promise> {
    //     Fetch(self.upcast(), input, init, comp, can_gc)
    // }
    //
    // // https://w3c.github.io/hr-time/#the-performance-attribute
    // fn Performance(&self) -> DomRoot<Performance> {
    //     self.performance.or_init(|| {
    //         let global_scope = self.upcast::<GlobalScope>();
    //         Performance::new(global_scope, self.navigation_start, CanGc::note())
    //     })
    // }

    // // https://html.spec.whatwg.org/multipage/#dom-origin
    // fn Origin(&self) -> USVString {
    //     USVString(
    //         self.upcast::<GlobalScope>()
    //             .origin()
    //             .immutable()
    //             .ascii_serialization(),
    //     )
    // }

    // // https://w3c.github.io/webappsec-secure-contexts/#dom-windoworworkerglobalscope-issecurecontext
    // fn IsSecureContext(&self) -> bool {
    //     self.upcast::<GlobalScope>().is_secure_context()
    // }

    // /// <https://html.spec.whatwg.org/multipage/#dom-structuredclone>
    // fn StructuredClone(
    //     &self,
    //     cx: JSContext,
    //     value: HandleValue,
    //     options: RootedTraceableBox<StructuredSerializeOptions>,
    //     retval: MutableHandleValue,
    // ) -> Fallible<()> {
    //     self.upcast::<GlobalScope>()
    //         .structured_clone(cx, value, options, retval)
    // }

    // /// <https://www.w3.org/TR/trusted-types/#dom-windoworworkerglobalscope-trustedtypes>
    // fn TrustedTypes(&self, can_gc: CanGc) -> DomRoot<TrustedTypePolicyFactory> {
    //     self.trusted_types.or_init(|| {
    //         let global_scope = self.upcast::<GlobalScope>();
    //         TrustedTypePolicyFactory::new(global_scope, can_gc)
    //     })
    // }
}

impl WorkerGlobalScope {
    #[allow(unsafe_code)]
    pub(crate) fn execute_script(&self, source: DOMString, can_gc: CanGc) {
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
            Ok(_) => {
                println!("Script executed successfully");
            },
            Err(_) => {
                
                // TODO: An error needs to be dispatched to the parent.
                // https://github.com/servo/servo/issues/6422
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
    }

    // pub(crate) fn new_script_pair(&self) -> (ScriptEventLoopSender, ScriptEventLoopReceiver) {
    //     let dedicated = self.downcast::<DedicatedWorkerGlobalScope>();
    //     if let Some(dedicated) = dedicated {
    //         dedicated.new_script_pair()
    //     } else {
    //         panic!("need to implement a sender for SharedWorker/ServiceWorker")
    //     }
    // }

    // /// Process a single event as if it were the next event
    // /// in the queue for this worker event-loop.
    // /// Returns a boolean indicating whether further events should be processed.
    // #[allow(unsafe_code)]
    // pub(crate) fn process_event(&self, msg: CommonScriptMsg) -> bool {
    //     if self.is_closing() {
    //         return false;
    //     }
    //     match msg {
    //         CommonScriptMsg::Task(_, task, _, _) => task.run_box(),
    //         CommonScriptMsg::CollectReports(reports_chan) => {
    //             let cx = self.get_cx();
    //             perform_memory_report(|ops| {
    //                 let reports = cx.get_reports(format!("url({})", self.get_url()), ops);
    //                 reports_chan.send(ProcessReports::new(reports));
    //             });
    //         },
    //         CommonScriptMsg::ReportCspViolations(_, violations) => {
    //             self.upcast::<GlobalScope>()
    //                 .report_csp_violations(violations, None, None);
    //         },
    //     }
    //     true
    // }
}
