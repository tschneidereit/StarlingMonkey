/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::{self, JoinHandle};

// use crossbeam_channel::{Receiver, Sender, unbounded};
// use devtools_traits::{DevtoolScriptControlMsg, ScriptToDevtoolsControlMsg, SourceInfo};
use dom_struct::dom_struct;
// use headers::{HeaderMapExt, ReferrerPolicy as ReferrerPolicyHeader};
use js::jsapi::{Heap, JS_AddInterruptCallback, JSContext, JSObject};
use js::jsval::UndefinedValue;
use js::rust::{CustomAutoRooter, CustomAutoRooterGuard, HandleValue};
// use net_traits::policy_container::PolicyContainer;
// use net_traits::{IpcSend, Metadata};
// use servo_rand::random;
use servo_url::{ImmutableOrigin, MutableOrigin, ServoUrl};
// use style::thread_state::{self, ThreadState};

// use crate::dom::abstractworker::SimpleWorkerErrorHandler;
// use crate::dom::abstractworker::WorkerScriptMsg;
use crate::dom::bindings::cell::DomRefCell;
use crate::dom::bindings::codegen::Bindings::DedicatedWorkerGlobalScopeBinding;
use crate::dom::bindings::codegen::Bindings::DedicatedWorkerGlobalScopeBinding::DedicatedWorkerGlobalScopeMethods;
// use crate::dom::bindings::codegen::Bindings::MessagePortBinding::StructuredSerializeOptions;
// use crate::dom::bindings::codegen::Bindings::WorkerBinding::WorkerType;
use crate::dom::bindings::error::{ErrorInfo, ErrorResult};
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::root::{DomRoot, RootCollection, ThreadLocalStackRoots};
use crate::dom::bindings::str::DOMString;
// use crate::dom::bindings::structuredclone;
use crate::dom::bindings::trace::{CustomTraceable, RootedTraceableBox};
use crate::dom::bindings::utils::define_all_exposed_interfaces;
// use crate::dom::csp::{Violation, parse_csp_list_from_metadata};
// use crate::dom::errorevent::ErrorEvent;
// use crate::dom::event::{Event, EventBubbles, EventCancelable};
// use crate::dom::eventtarget::EventTarget;
use crate::dom::globalscope::GlobalScope;
// use crate::dom::messageevent::MessageEvent;
// use crate::dom::reportingendpoint::ReportingEndpoint;
// use crate::dom::types::DebuggerGlobalScope;
// use crate::dom::worker::TrustedWorkerAddress;
// use crate::dom::worker::Worker;
use crate::dom::workerglobalscope::WorkerGlobalScope;
// use crate::fetch::{CspViolationsProcessor, load_whole_resource};
// use crate::messaging::{CommonScriptMsg, ScriptEventLoopReceiver, ScriptEventLoopSender};
use crate::realms::{AlreadyInRealm, InRealm, enter_realm};
use crate::script_runtime::ScriptThreadEventCategory::WorkerEvent;
use crate::script_runtime::{
    CanGc, IntroductionType, JSContext as SafeJSContext, Runtime, ThreadSafeJSContext,
};
// use crate::task_queue::{QueuedTask, QueuedTaskConversion, TaskQueue};
// use crate::task_source::{SendableTaskSource, TaskSourceName};

// impl QueuedTaskConversion for DedicatedWorkerScriptMsg {
//     fn task_source_name(&self) -> Option<&TaskSourceName> {
//         let common_worker_msg = match self {
//             DedicatedWorkerScriptMsg::CommonWorker(_, common_worker_msg) => common_worker_msg,
//             _ => return None,
//         };
//         let script_msg = match common_worker_msg {
//             WorkerScriptMsg::Common(script_msg) => script_msg,
//             _ => return None,
//         };
//         match script_msg {
//             CommonScriptMsg::Task(_category, _boxed, _pipeline_id, source_name) => {
//                 Some(source_name)
//             },
//             _ => None,
//         }
//     }
// 
//     fn into_queued_task(self) -> Option<QueuedTask> {
//         let (worker, common_worker_msg) = match self {
//             DedicatedWorkerScriptMsg::CommonWorker(worker, common_worker_msg) => {
//                 (worker, common_worker_msg)
//             },
//             _ => return None,
//         };
//         let script_msg = match common_worker_msg {
//             WorkerScriptMsg::Common(script_msg) => script_msg,
//             _ => return None,
//         };
//         let (category, boxed, pipeline_id, task_source) = match script_msg {
//             CommonScriptMsg::Task(category, boxed, pipeline_id, task_source) => {
//                 (category, boxed, pipeline_id, task_source)
//             },
//             _ => return None,
//         };
//         Some((Some(worker), category, boxed, pipeline_id, task_source))
//     }
// 
//     fn from_queued_task(queued_task: QueuedTask) -> Self {
//         let (worker, category, boxed, pipeline_id, task_source) = queued_task;
//         let script_msg = CommonScriptMsg::Task(category, boxed, pipeline_id, task_source);
//         DedicatedWorkerScriptMsg::CommonWorker(worker.unwrap(), WorkerScriptMsg::Common(script_msg))
//     }
// 
//     fn inactive_msg() -> Self {
//         // Inactive is only relevant in the context of a browsing-context event-loop.
//         panic!("Workers should never receive messages marked as inactive");
//     }
// 
//     fn wake_up_msg() -> Self {
//         DedicatedWorkerScriptMsg::WakeUp
//     }
// 
//     fn is_wake_up(&self) -> bool {
//         matches!(self, DedicatedWorkerScriptMsg::WakeUp)
//     }
// }

// unsafe_no_jsmanaged_fields!(TaskQueue<DedicatedWorkerScriptMsg>);

// https://html.spec.whatwg.org/multipage/#dedicatedworkerglobalscope
#[dom_struct]
pub struct DedicatedWorkerGlobalScope {
    workerglobalscope: WorkerGlobalScope,
    // #[ignore_malloc_size_of = "Defined in std"]
    // task_queue: TaskQueue<DedicatedWorkerScriptMsg>,
    // own_sender: Sender<DedicatedWorkerScriptMsg>,
    // #[ignore_malloc_size_of = "Can't measure trait objects"]
    // /// Sender to the parent thread.
    // parent_event_loop_sender: ScriptEventLoopSender,
}

// impl WorkerEventLoopMethods for DedicatedWorkerGlobalScope {
//     // type WorkerMsg = DedicatedWorkerScriptMsg;
//     // type ControlMsg = DedicatedWorkerControlMsg;
//     type Event = MixedMessage;
//
//     fn task_queue(&self) -> &TaskQueue<DedicatedWorkerScriptMsg> {
//         &self.task_queue
//     }
//
//     fn handle_event(&self, event: MixedMessage, can_gc: CanGc) -> bool {
//         self.handle_mixed_message(event, can_gc)
//     }
//
//     // fn handle_worker_post_event(&self, worker: &TrustedWorkerAddress) -> Option<AutoWorkerReset> {
//     //     let ar = AutoWorkerReset::new(self, worker.clone());
//     //     Some(ar)
//     // }
//     //
//     // fn from_control_msg(msg: DedicatedWorkerControlMsg) -> MixedMessage {
//     //     MixedMessage::Control(msg)
//     // }
//     //
//     // fn from_worker_msg(msg: DedicatedWorkerScriptMsg) -> MixedMessage {
//     //     MixedMessage::Worker(msg)
//     // }
//     //
//     // fn from_devtools_msg(msg: DevtoolScriptControlMsg) -> MixedMessage {
//     //     MixedMessage::Devtools(msg)
//     // }
//
//     fn from_timer_msg() -> MixedMessage {
//         MixedMessage::Timer
//     }
// }

impl DedicatedWorkerGlobalScope {
    #[allow(clippy::too_many_arguments)]
    fn new_inherited(
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_name: DOMString,
        // worker_type: WorkerType,
        worker_url: ServoUrl,
        // from_devtools_receiver: Receiver<DevtoolScriptControlMsg>,
        runtime: Runtime,
        // parent_event_loop_sender: ScriptEventLoopSender,
        // own_sender: Sender<DedicatedWorkerScriptMsg>,
        // receiver: Receiver<DedicatedWorkerScriptMsg>,
        // insecure_requests_policy: InsecureRequestsPolicy,
    ) -> DedicatedWorkerGlobalScope {
        DedicatedWorkerGlobalScope {
            workerglobalscope: WorkerGlobalScope::new_inherited(
                origin,
                creation_url,
                worker_name,
                // worker_type,
                worker_url,
                runtime,
                // from_devtools_receiver,
                // insecure_requests_policy,
            ),
            // task_queue: TaskQueue::new(receiver, own_sender.clone()),
            // own_sender,
            // parent_event_loop_sender,
            // worker: DomRefCell::new(None),
        }
    }

    #[allow(unsafe_code, clippy::too_many_arguments)]
    pub fn new(
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_name: DOMString,
        worker_url: ServoUrl,
        runtime: Runtime,
    ) -> DomRoot<DedicatedWorkerGlobalScope> {
        let cx = runtime.cx();
        let scope = Box::new(DedicatedWorkerGlobalScope::new_inherited(
            origin,
            creation_url,
            worker_name,
            worker_url,
            runtime,
        ));
        unsafe {
            DedicatedWorkerGlobalScopeBinding::Wrap::<crate::DomTypeHolder>(
                SafeJSContext::from_ptr(cx),
                scope,
            )
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#run-a-worker>
    #[allow(unsafe_code, clippy::too_many_arguments)]
    pub fn run_worker_scope(
        origin: MutableOrigin,
        creation_url: ServoUrl,
        worker_url: ServoUrl,
        worker_name: String,
    ) -> DomRoot<DedicatedWorkerGlobalScope> {
        let serialized_worker_url = worker_url.to_string();
        let runtime = Runtime::new();
        let global = DedicatedWorkerGlobalScope::new(
            origin,
            creation_url,
            DOMString::from_string(worker_name),
            worker_url,
            runtime,
        );
        let scope = global.upcast::<WorkerGlobalScope>();
        {
            let realm = enter_realm(scope);
            define_all_exposed_interfaces(
                global.upcast(),
                InRealm::entered(&realm),
                CanGc::note(),
            );
        }

        global
    }

    pub fn execute_script(&self, source: DOMString, can_gc: CanGc) {
        self.upcast::<WorkerGlobalScope>().execute_script(source, can_gc)
    }

    // pub(crate) fn event_loop_sender(&self) -> Option<ScriptEventLoopSender> {
    //     Some(ScriptEventLoopSender::DedicatedWorker {
    //         sender: self.own_sender.clone(),
    //         main_thread_worker: self.worker.borrow().clone()?,
    //     })
    // }
    // 
    // pub(crate) fn new_script_pair(&self) -> (ScriptEventLoopSender, ScriptEventLoopReceiver) {
    //     let (sender, receiver) = unbounded();
    //     let main_thread_worker = self.worker.borrow().as_ref().unwrap().clone();
    //     (
    //         ScriptEventLoopSender::DedicatedWorker {
    //             sender,
    //             main_thread_worker,
    //         },
    //         ScriptEventLoopReceiver::DedicatedWorker(receiver),
    //     )
    // }

    // fn handle_script_event(&self, msg: WorkerScriptMsg, can_gc: CanGc) {
    //     match msg {
    //         // WorkerScriptMsg::DOMMessage { origin, data } => {
    //         //     let scope = self.upcast::<WorkerGlobalScope>();
    //         //     let target = self.upcast();
    //         //     let _ac = enter_realm(self);
    //         //     rooted!(in(*scope.get_cx()) let mut message = UndefinedValue());
    //         //     if let Ok(ports) =
    //         //         structuredclone::read(scope.upcast(), *data, message.handle_mut())
    //         //     {
    //         //         MessageEvent::dispatch_jsval(
    //         //             target,
    //         //             scope.upcast(),
    //         //             message.handle(),
    //         //             Some(&origin.ascii_serialization()),
    //         //             None,
    //         //             ports,
    //         //             can_gc,
    //         //         );
    //         //     } else {
    //         //         MessageEvent::dispatch_error(target, scope.upcast(), can_gc);
    //         //     }
    //         // },
    //         WorkerScriptMsg::Common(msg) => {
    //             self.upcast::<WorkerGlobalScope>().process_event(msg);
    //         },
    //     }
    // }

    // fn handle_mixed_message(&self, msg: MixedMessage, can_gc: CanGc) -> bool {
    //     if self.upcast::<WorkerGlobalScope>().is_closing() {
    //         return false;
    //     }
    //     // FIXME(#26324): `self.worker` is None in devtools messages.
    //     match msg {
    //         // MixedMessage::Devtools(msg) => match msg {
    //         //     DevtoolScriptControlMsg::EvaluateJS(_pipe_id, string, sender) => {
    //         //         devtools::handle_evaluate_js(self.upcast(), string, sender, can_gc)
    //         //     },
    //         //     DevtoolScriptControlMsg::WantsLiveNotifications(_pipe_id, bool_val) => {
    //         //         devtools::handle_wants_live_notifications(self.upcast(), bool_val)
    //         //     },
    //         //     _ => debug!("got an unusable devtools control message inside the worker!"),
    //         // },
    //         MixedMessage::Worker(DedicatedWorkerScriptMsg::CommonWorker(linked_worker, msg)) => {
    //             let _ar = AutoWorkerReset::new(self, linked_worker);
    //             self.handle_script_event(msg, can_gc);
    //         },
    //         // MixedMessage::Worker(DedicatedWorkerScriptMsg::WakeUp) => {},
    //         // MixedMessage::Control(DedicatedWorkerControlMsg::Exit) => {
    //         //     return false;
    //         // },
    //         MixedMessage::Timer => {},
    //     }
    //     true
    // }
}

impl DedicatedWorkerGlobalScopeMethods<crate::DomTypeHolder> for DedicatedWorkerGlobalScope {
}
