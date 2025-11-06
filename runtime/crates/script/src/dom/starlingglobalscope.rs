/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use base::id::PipelineId;
use crossbeam_channel::{unbounded, Receiver, Sender};
use ipc_channel::ipc;
use dom_struct::dom_struct;
use js::jsval::UndefinedValue;
use js::rust::ParentRuntime;
use rustc_hash::FxHashSet;
use constellation_traits::ScriptToConstellationChan;
use embedder_traits::{EmbedderMsg, EmbedderProxy, EventLoopWaker, ScriptToEmbedderChan};
use net_traits::ResourceThreads;
use profile_traits::{generic_channel, mem as profile_mem, time as profile_time};
use servo_url::{MutableOrigin, ServoUrl};
use storage_traits::StorageThreads;
use crate::dom::abstractworker::WorkerScriptMsg;
use crate::dom::abstractworkerglobalscope::{run_worker_event_loop, WorkerEventLoopMethods};
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
use crate::dom::bindings::trace::{CustomTraceable, RootedTraceableBox};
use crate::dom::bindings::utils::define_all_exposed_interfaces;
use crate::dom::globalscope::GlobalScope;
use crate::dom::workerglobalscope::WorkerGlobalScope;
use crate::messaging::{MainThreadScriptMsg, ScriptEventLoopSender};
use crate::realms::{InRealm, enter_realm};
use crate::script_runtime::{CanGc, IntroductionType, JSContext, JSContextHelper, Runtime};
use crate::task_queue::TaskQueue;
use crate::task_source::{SendableTaskSource, TaskSourceName};

unsafe_no_jsmanaged_fields!(TaskQueue<WorkerScriptMsg>);

// https://html.spec.whatwg.org/multipage/#the-workerglobalscope-common-interface
#[dom_struct]
pub struct StarlingGlobalScope {
    globalscope: WorkerGlobalScope,
    #[ignore_malloc_size_of = "Defined in std"]
    task_queue: TaskQueue<WorkerScriptMsg>,
    own_sender: Sender<WorkerScriptMsg>,
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
        // devtools_chan: Option<IpcSender<ScriptToDevtoolsControlMsg>>,
        mem_profiler_chan: profile_mem::ProfilerChan,
        time_profiler_chan: profile_time::ProfilerChan,
        script_to_constellation_chan: ScriptToConstellationChan,
        script_to_embedder_chan: ScriptToEmbedderChan,
        resource_threads: ResourceThreads,
        storage_threads: StorageThreads,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
        own_sender: Sender<WorkerScriptMsg>,
        receiver: Receiver<WorkerScriptMsg>,
    ) -> DomRoot<Self> {
        let cx = runtime.cx();
        let scope = Box::new(Self::new_inherited(
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
            worker_name,
            worker_url,
            runtime,
            own_sender,
            receiver,
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
        // devtools_chan: Option<IpcSender<ScriptToDevtoolsControlMsg>>,
        mem_profiler_chan: profile_mem::ProfilerChan,
        time_profiler_chan: profile_time::ProfilerChan,
        script_to_constellation_chan: ScriptToConstellationChan,
        script_to_embedder_chan: ScriptToEmbedderChan,
        resource_threads: ResourceThreads,
        storage_threads: StorageThreads,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
        own_sender: Sender<WorkerScriptMsg>,
        receiver: Receiver<WorkerScriptMsg>,
    ) -> Self {
        Self {
            globalscope: WorkerGlobalScope::new_inherited(
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
                runtime.microtask_queue.clone(),
                // false,
            ),
            task_queue: TaskQueue::new(receiver, own_sender.clone()),
            own_sender,
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

    pub(crate) fn event_loop_sender(&self) -> Option<ScriptEventLoopSender> {
        Some(ScriptEventLoopSender::Starling(self.own_sender.clone()))
    }
}

pub(crate) enum MixedMessage {
    Worker(WorkerScriptMsg),
    Timer,
}

impl WorkerEventLoopMethods for StarlingGlobalScope {
    type WorkerMsg = WorkerScriptMsg;
    type Event = MixedMessage;

    fn task_queue(&self) -> &TaskQueue<WorkerScriptMsg> {
        &self.task_queue
    }

    fn handle_event(&self, event: MixedMessage, can_gc: CanGc) -> bool {
        self.handle_mixed_message(event, can_gc)
    }

    fn from_worker_msg(msg: WorkerScriptMsg) -> MixedMessage {
        MixedMessage::Worker(msg)
    }

    fn from_timer_msg() -> MixedMessage {
        MixedMessage::Timer
    }
}

struct DefaultEventLoopWaker;

impl EventLoopWaker for DefaultEventLoopWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(DefaultEventLoopWaker)
    }
}

fn create_embedder_channel(
    event_loop_waker: Box<dyn EventLoopWaker>,
) -> (EmbedderProxy, Receiver<EmbedderMsg>) {
    let (sender, receiver) = unbounded();
    (
        EmbedderProxy {
            sender,
            event_loop_waker,
        },
        receiver,
    )
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
        let (self_sender, self_receiver) = unbounded();
        let runtime = Runtime::new(Some(SendableTaskSource {
            sender: ScriptEventLoopSender::Starling(self_sender.clone()),
            pipeline_id,
            name: TaskSourceName::Networking,
            canceller: Default::default(),
        }));
        let time_profiler_chan = profile::time::Profiler::create(
            &None, // &opts.time_profiling,
            None, //opts.time_profiler_trace_path.clone(),
        );
        let mem_profiler_chan = profile::mem::Profiler::create();

        let (constellation_sender, constellation_receiver) =
            generic_channel::channel(time_profiler_chan.clone()).unwrap();
        let script_to_constellation_chan = ScriptToConstellationChan {
            sender: constellation_sender,
            pipeline_id,
        };

        let event_loop_waker: Box<dyn EventLoopWaker> = Box::new(DefaultEventLoopWaker);
        let (embedder_proxy, embedder_receiver) = create_embedder_channel(event_loop_waker.clone());
        let embedder_chan = embedder_proxy.sender.clone();
        let eventloop_waker = event_loop_waker.clone();
        let script_to_embedder_chan = ScriptToEmbedderChan::new(embedder_chan, eventloop_waker);
        let (storage_sender, storage_receiver) =
            generic_channel::channel(time_profiler_chan.clone()).unwrap();
        let storage_threads: StorageThreads = StorageThreads::new(storage_sender);
        let (core_sender, _) = ipc::channel().unwrap();
        let mock_resource_threads = ResourceThreads::new(core_sender);

        let global = Self::new(
            pipeline_id,
            // None,
            mem_profiler_chan,
            time_profiler_chan,
            script_to_constellation_chan,
            script_to_embedder_chan,
            mock_resource_threads,
            storage_threads,
            origin,
            creation_url,
            DOMString::from_string(worker_name),
            worker_url,
            runtime,
            self_sender,
            self_receiver,
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
    pub fn execute_script(&self, source: &str, can_gc: CanGc) {
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
            source,
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

        self.globalscope.as_global_scope().perform_a_microtask_checkpoint(can_gc);
    }

    fn handle_mixed_message(&self, msg: MixedMessage, can_gc: CanGc) -> bool {
        if self.upcast::<WorkerGlobalScope>().is_closing() {
            return false;
        }
        // FIXME(#26324): `self.worker` is None in devtools messages.
        match msg {
            // MixedMessage::Devtools(msg) => match msg {
            //     DevtoolScriptControlMsg::EvaluateJS(_pipe_id, string, sender) => {
            //         devtools::handle_evaluate_js(self.upcast(), string, sender, can_gc)
            //     },
            //     DevtoolScriptControlMsg::WantsLiveNotifications(_pipe_id, bool_val) => {
            //         devtools::handle_wants_live_notifications(self.upcast(), bool_val)
            //     },
            //     _ => debug!("got an unusable devtools control message inside the worker!"),
            // },
            MixedMessage::Worker(WorkerScriptMsg::Common(msg)) => {
                self.upcast::<WorkerGlobalScope>().process_event(msg);
            },
            MixedMessage::Timer => {},
            MixedMessage::Worker(WorkerScriptMsg::WakeUp) => {},
        }
        true
    }

    #[allow(unsafe_code)]
    pub fn process_events(&self, _can_gc: CanGc) {
        run_worker_event_loop(&*self, CanGc::note());
    }
}
