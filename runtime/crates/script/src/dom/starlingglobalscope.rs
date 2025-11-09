/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::sync::Arc;

use crate::dom::abstractworker::WorkerScriptMsg;
use crate::dom::abstractworkerglobalscope::{run_worker_event_loop, WorkerEventLoopMethods};
use crate::dom::bindings::codegen::Bindings::StarlingGlobalScopeBinding;
use crate::dom::bindings::import::base::SafeJSContext;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::reflector::DomObject;
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::str::DOMString;
use crate::dom::bindings::trace::CustomTraceable;
use crate::dom::bindings::utils::define_all_exposed_interfaces;
use crate::dom::globalscope::GlobalScope;
use crate::dom::workerglobalscope::WorkerGlobalScope;
use crate::messaging::ScriptEventLoopSender;
use crate::realms::{enter_realm, InRealm};
use crate::script_runtime::{CanGc, Runtime};
use crate::task_queue::TaskQueue;
use crate::task_source::{SendableTaskSource, TaskSourceName};
use base::id::PipelineId;
use constellation_traits::{ScriptToConstellationChan, WorkerGlobalScopeInit};
use crossbeam_channel::{unbounded, Receiver, Sender};
use devtools_traits::WorkerId;
use dom_struct::dom_struct;
use embedder_traits::{EmbedderMsg, EmbedderProxy, EventLoopWaker, ScriptToEmbedderChan};
use ipc_channel::ipc;
use net_traits::request::InsecureRequestsPolicy;
use net_traits::ResourceThreads;
use profile_traits::generic_channel;
use servo_url::{MutableOrigin, ServoUrl};
use storage_traits::StorageThreads;
use uuid::Uuid;

unsafe_no_jsmanaged_fields!(TaskQueue<WorkerScriptMsg>);

// https://html.spec.whatwg.org/multipage/#the-workerglobalscope-common-interface
#[dom_struct]
pub struct StarlingGlobalScope {
    globalscope: WorkerGlobalScope,
    #[ignore_malloc_size_of = "Defined in std"]
    task_queue: TaskQueue<WorkerScriptMsg>,
    own_sender: Sender<WorkerScriptMsg>,
}

impl StarlingGlobalScope {

    #[allow(unsafe_code, clippy::too_many_arguments)]
    pub(crate) fn new(
        init: WorkerGlobalScopeInit,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
        own_sender: Sender<WorkerScriptMsg>,
        receiver: Receiver<WorkerScriptMsg>,
    ) -> DomRoot<Self> {
        let cx = runtime.cx();
        let scope = Box::new(Self::new_inherited(
            init,
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
        init: WorkerGlobalScopeInit,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
        own_sender: Sender<WorkerScriptMsg>,
        receiver: Receiver<WorkerScriptMsg>,
    ) -> Self {
        Self {
            globalscope: WorkerGlobalScope::new_inherited(
                init,
                worker_name,
                worker_url,
                runtime,
                Arc::new(false.into()),
                InsecureRequestsPolicy::Upgrade,
            ),
            task_queue: TaskQueue::new(receiver, own_sender.clone()),
            own_sender,
        }
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
        let init = WorkerGlobalScopeInit {
            pipeline_id,
            // devtools_chan,
            origin: origin.immutable().clone(),
            creation_url,
            mem_profiler_chan,
            time_profiler_chan,
            to_devtools_sender: None,
            from_devtools_sender: None,
            script_to_constellation_chan,
            script_to_embedder_chan,
            resource_threads: mock_resource_threads,
            storage_threads,
            worker_id: WorkerId(Uuid::default()),
            inherited_secure_context: None,
        };

        let global = Self::new(
            init,
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
        self.globalscope.execute_script(source.into(), can_gc);
        self.globalscope.upcast::<GlobalScope>().perform_a_microtask_checkpoint(can_gc);
    }

    fn handle_mixed_message(&self, msg: MixedMessage, _can_gc: CanGc) -> bool {
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
