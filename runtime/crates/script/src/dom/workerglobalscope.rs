/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use std::cell::{RefCell, RefMut};
use std::rc::Rc;
use std::time::Duration;
use base::id::PipelineId;
use js::gc::HandleValue;
use js::rust::{HandleObject, MutableHandleObject};
use constellation_traits::ScriptToConstellationChan;
use dom_struct::dom_struct;
use embedder_traits::ScriptToEmbedderChan;
use net_traits::ResourceThreads;
use profile_traits::{ipc as profile_ipc, mem as profile_mem, time as profile_time};
use script_bindings::codegen::GenericBindings::VoidFunctionBinding::VoidFunction;
use script_bindings::codegen::GenericBindings::WorkerGlobalScopeBinding::WorkerGlobalScopeMethods;
use script_bindings::error::Fallible;
use script_bindings::root::DomRoot;
use script_bindings::script_runtime::CanGc;
use servo_url::{MutableOrigin, ServoUrl};
use storage_traits::StorageThreads;
use timers::TimerScheduler;
use crate::dom::bindings::codegen::DomTypeHolder::DomTypeHolder;
use crate::dom::bindings::codegen::UnionTypes::{TrustedScriptOrString, TrustedScriptOrStringOrFunction};
use crate::dom::bindings::inheritance::Castable;
use crate::dom::globalscope::GlobalScope;
use crate::messaging::CommonScriptMsg;
use crate::microtask::MicrotaskQueue;
use crate::script_runtime::JSContext;
use crate::timers::{IsInterval, TimerCallback};

#[dom_struct]
pub struct WorkerGlobalScope {
    global_scope: GlobalScope,

    /// A [`TimerScheduler`] used to schedule timers for this [`WorkerGlobalScope`].
    /// Timers are handled in the service worker event loop.
    #[no_trace]
    timer_scheduler: RefCell<TimerScheduler>,
}

impl WorkerGlobalScope {

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_inherited(
        pipeline_id: PipelineId,
        // devtools_chan: Option<IpcSender<ScriptToDevtoolsControlMsg>>,
        mem_profiler_chan: profile_mem::ProfilerChan,
        time_profiler_chan: profile_time::ProfilerChan,
        script_to_constellation_chan: ScriptToConstellationChan,
        script_to_embedder_chan: ScriptToEmbedderChan,
        resource_threads: ResourceThreads,
        storage_threads: StorageThreads,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        microtask_queue: Rc<MicrotaskQueue>,
    ) -> Self {
        Self {
            global_scope: GlobalScope::new_inherited(
                pipeline_id,
                // devtools_chan,
                mem_profiler_chan,
                time_profiler_chan,
                script_to_constellation_chan,
                script_to_embedder_chan,
                resource_threads,
                storage_threads,
                origin,
                creation_url,
                None,
                microtask_queue,
                None,
                // false,
            ),
            timer_scheduler: RefCell::default(),
        }
    }

    /// Get the JS context.
    pub(crate) fn get_cx() -> JSContext {
        GlobalScope::get_cx()
    }

    pub(crate) fn is_closing(&self) -> bool {
        false // self.closing.load(Ordering::SeqCst)
    }

    pub(crate) fn as_global_scope(&self) -> &GlobalScope {
        self.upcast::<GlobalScope>()
    }

    /// Get a mutable reference to the [`TimerScheduler`] for this [`ServiceWorkerGlobalScope`].
    pub(crate) fn timer_scheduler(&self) -> RefMut<'_, TimerScheduler> {
        self.timer_scheduler.borrow_mut()
    }

    /// Process a single event as if it were the next event
    /// in the queue for this worker event-loop.
    /// Returns a boolean indicating whether further events should be processed.
    #[allow(unsafe_code)]
    pub(crate) fn process_event(&self, msg: CommonScriptMsg) -> bool {
        if self.is_closing() {
            return false;
        }
        match msg {
            CommonScriptMsg::Task(_, task, _, _) => task.run_box(),
            // CommonScriptMsg::CollectReports(reports_chan) => {
            //     let cx = self.get_cx();
            //     perform_memory_report(|ops| {
            //         let reports = cx.get_reports(format!("url({})", self.get_url()), ops);
            //         reports_chan.send(ProcessReports::new(reports));
            //     });
            // },
            // CommonScriptMsg::ReportCspViolations(_, violations) => {
            //     self.upcast::<GlobalScope>()
            //         .report_csp_violations(violations, None, None);
            // },
        }
        true
    }
}

impl WorkerGlobalScopeMethods<crate::DomTypeHolder> for WorkerGlobalScope {
    // https://html.spec.whatwg.org/multipage/#dom-workerglobalscope-self
    fn Self_(&self) -> DomRoot<WorkerGlobalScope> {
        DomRoot::from_ref(self)
    }

    // https://html.spec.whatwg.org/multipage/#dom-windowtimers-settimeout
    fn SetTimeout(
        &self,
        _cx: JSContext,
        callback: TrustedScriptOrStringOrFunction,
        timeout: i32,
        args: Vec<HandleValue>,
        can_gc: CanGc,
    ) -> Fallible<i32> {
        let callback = match callback {
            TrustedScriptOrStringOrFunction::String(i) => {
                TimerCallback::StringTimerCallback(TrustedScriptOrString::String(i))
            },
            TrustedScriptOrStringOrFunction::TrustedScript(i) => {
                TimerCallback::StringTimerCallback(TrustedScriptOrString::TrustedScript(i))
            },
            TrustedScriptOrStringOrFunction::Function(i) => TimerCallback::FunctionTimerCallback(i),
        };
        self.upcast::<GlobalScope>().set_timeout_or_interval(
            callback,
            args,
            Duration::from_millis(timeout.max(0) as u64),
            IsInterval::NonInterval,
            can_gc,
        )
    }

    // https://html.spec.whatwg.org/multipage/#dom-windowtimers-cleartimeout
    fn ClearTimeout(&self, handle: i32) {
        self.upcast::<GlobalScope>()
            .clear_timeout_or_interval(handle);
    }

    // https://html.spec.whatwg.org/multipage/#dom-windowtimers-setinterval
    fn SetInterval(
        &self,
        _cx: JSContext,
        callback: TrustedScriptOrStringOrFunction,
        timeout: i32,
        args: Vec<HandleValue>,
        can_gc: CanGc,
    ) -> Fallible<i32> {
        let callback = match callback {
            TrustedScriptOrStringOrFunction::String(i) => {
                TimerCallback::StringTimerCallback(TrustedScriptOrString::String(i))
            },
            TrustedScriptOrStringOrFunction::TrustedScript(i) => {
                TimerCallback::StringTimerCallback(TrustedScriptOrString::TrustedScript(i))
            },
            TrustedScriptOrStringOrFunction::Function(i) => TimerCallback::FunctionTimerCallback(i),
        };
        self.upcast::<GlobalScope>().set_timeout_or_interval(
            callback,
            args,
            Duration::from_millis(timeout.max(0) as u64),
            IsInterval::Interval,
            can_gc,
        )
    }

    // https://html.spec.whatwg.org/multipage/#dom-windowtimers-clearinterval
    fn ClearInterval(&self, handle: i32) {
        self.ClearTimeout(handle);
    }

    fn QueueMicrotask(&self, r#callback: Rc<VoidFunction<DomTypeHolder>>) {
        self.upcast::<GlobalScope>()
            .queue_function_as_microtask(callback);
    }
}

