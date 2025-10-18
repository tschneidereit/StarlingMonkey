/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::{Cell, OnceCell, Ref};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::CStr;
use std::ops::Index;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{mem, ptr};

use dom_struct::dom_struct;
use js::glue::{IsWrapper, UnwrapObjectDynamic};
use js::jsapi::{
    Compile1, CurrentGlobalOrNull, DelazificationOption, GetNonCCWObjectGlobal, HandleObject, Heap,
    InstantiateOptions, JSContext, JSObject, JSScript, SetScriptPrivate,
};
use js::jsval::{PrivateValue, UndefinedValue};
use js::panic::maybe_resume_unwind;
use js::rust::wrappers::{JS_ExecuteScript, JS_GetScriptPrivate};
use js::rust::{
    CompileOptionsWrapper, CustomAutoRooter, CustomAutoRooterGuard, HandleValue,
    MutableHandleValue, ParentRuntime, Runtime, get_object_class, transform_str_to_source_text,
};
use js::{JSCLASS_IS_DOMJSCLASS, JSCLASS_IS_GLOBAL};
use script_bindings::interfaces::GlobalScopeHelpers;
use script_bindings::reflector::Reflector;
use servo_url::{ImmutableOrigin, MutableOrigin, ServoUrl};
use crate::base::id::PipelineId;
use super::bindings::trace::{HashMapTracedValues, RootedTraceableBox};
use crate::dom::bindings::cell::{DomRefCell, RefMut};
use crate::dom::bindings::codegen::Bindings::VoidFunctionBinding::VoidFunction;
// use crate::dom::bindings::codegen::Bindings::EventSourceBinding::EventSource_Binding::EventSourceMethods;
// use crate::dom::bindings::codegen::Bindings::FunctionBinding::Function;
// use crate::dom::bindings::codegen::Bindings::VoidFunctionBinding::VoidFunction;
use crate::dom::bindings::conversions::{root_from_object, root_from_object_static};
use crate::dom::bindings::error::{Error, ErrorInfo, report_pending_exception};
use crate::dom::bindings::frozenarray::CachedFrozenArray;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::refcounted::Trusted;
// use crate::dom::bindings::refcounted::TrustedPromise;
use crate::dom::bindings::reflector::{DomGlobal, DomObject};
use crate::dom::bindings::root::{Dom, DomRoot, MutNullableDom};
use crate::dom::bindings::settings_stack::{AutoEntryScript, entry_global, incumbent_global};
use crate::dom::bindings::str::DOMString;
// use crate::dom::bindings::structuredclone;
use crate::dom::bindings::trace::CustomTraceable;
use crate::dom::bindings::weakref::{DOMTracker, WeakRef};
use crate::dom::types::StarlingGlobalScope;
// use crate::dom::errorevent::ErrorEvent;
// use crate::dom::event::{Event, EventBubbles, EventCancelable};
// use crate::dom::eventsource::EventSource;
// use crate::dom::eventtarget::EventTarget;
// use crate::dom::performance::Performance;
// use crate::dom::performanceobserver::VALID_ENTRY_TYPES;
// use crate::dom::promise::Promise;
// use crate::dom::readablestream::{CrossRealmTransformReadable, ReadableStream};
// use crate::dom::reportingobserver::ReportingObserver;
// use crate::dom::types::{DebuggerGlobalScope, MessageEvent};
use crate::microtask::{Microtask, MicrotaskQueue, UserMicrotask};
use crate::realms::{InRealm, enter_realm};
// use crate::script_module::{
//     DynamicModuleList, ImportMap, ModuleScript, ModuleTree, ResolvedModule, ScriptFetchOptions,
// };
use crate::script_runtime::{CanGc, JSContext as SafeJSContext, ThreadSafeJSContext};
// use crate::task_manager::TaskManager;
// use crate::task_source::SendableTaskSource;
// use crate::timers::{
//     IsInterval, OneshotTimerCallback, OneshotTimerHandle, OneshotTimers, TimerCallback,
//     TimerEventId, TimerSource,
// };



#[derive(JSTraceable, MallocSizeOf)]
pub(crate) enum SourceCode {
    Text(#[conditional_malloc_size_of] Rc<DOMString>),
    // Compiled(CompiledSourceCode),
}

#[dom_struct]
pub struct GlobalScope {
    reflector: Reflector,
    // eventtarget: EventTarget,
    // crypto: MutNullableDom<Crypto>,

    // /// A [`TaskManager`] for this [`GlobalScope`].
    // task_manager: OnceCell<TaskManager>,

    /// Pipeline id associated with this global.
    #[no_trace]
    pipeline_id: PipelineId,
    // 
    // /// Timers (milliseconds) used by the Console API.
    // console_timers: DomRefCell<HashMap<DOMString, Instant>>,
    // 
    // /// module map is used when importing JavaScript modules
    // /// <https://html.spec.whatwg.org/multipage/#concept-settings-object-module-map>
    // #[ignore_malloc_size_of = "mozjs"]
    // module_map: DomRefCell<HashMapTracedValues<ServoUrl, Rc<ModuleTree>>>,
    // 
    // #[ignore_malloc_size_of = "mozjs"]
    // inline_module_map: DomRefCell<HashMap<ScriptId, Rc<ModuleTree>>>,
    // 
    // /// <https://html.spec.whatwg.org/multipage/#in-error-reporting-mode>
    // in_error_reporting_mode: Cell<bool>,
    // 
    // /// The mechanism by which time-outs and intervals are scheduled.
    // /// <https://html.spec.whatwg.org/multipage/#timers>
    // timers: OnceCell<OneshotTimers>,

    /// The origin of the globalscope
    #[no_trace]
    origin: MutableOrigin,

    /// <https://html.spec.whatwg.org/multipage/#concept-environment-creation-url>
    #[no_trace]
    creation_url: ServoUrl,

    /// <https://html.spec.whatwg.org/multipage/#concept-environment-top-level-creation-url>
    #[no_trace]
    top_level_creation_url: Option<ServoUrl>,

    /// The microtask queue associated with this global.
    ///
    /// It is refcounted because windows in the same script thread share the
    /// same microtask queue.
    ///
    /// <https://html.spec.whatwg.org/multipage/#microtask-queue>
    #[ignore_malloc_size_of = "Rc<T> is hard"]
    microtask_queue: Rc<MicrotaskQueue>,
    // 
    // /// Vector storing references of all eventsources.
    // event_source_tracker: DOMTracker<EventSource>,

    /// Storage for watching rejected promises waiting for some client to
    /// consume their rejection.
    /// Promises in this list have been rejected in the last turn of the
    /// event loop without the rejection being handled.
    /// Note that this can contain nullptrs in place of promises removed because
    /// they're consumed before it'd be reported.
    ///
    /// <https://html.spec.whatwg.org/multipage/#about-to-be-notified-rejected-promises-list>
    #[ignore_malloc_size_of = "mozjs"]
    // `Heap` values must stay boxed, as they need semantics like `Pin`
    // (that is, they cannot be moved).
    #[allow(clippy::vec_box)]
    uncaught_rejections: DomRefCell<Vec<Box<Heap<*mut JSObject>>>>,

    /// Promises in this list have previously been reported as rejected
    /// (because they were in the above list), but the rejection was handled
    /// in the last turn of the event loop.
    ///
    /// <https://html.spec.whatwg.org/multipage/#outstanding-rejected-promises-weak-set>
    #[ignore_malloc_size_of = "mozjs"]
    // `Heap` values must stay boxed, as they need semantics like `Pin`
    // (that is, they cannot be moved).
    #[allow(clippy::vec_box)]
    consumed_rejections: DomRefCell<Vec<Box<Heap<*mut JSObject>>>>,

    // https://w3c.github.io/performance-timeline/#supportedentrytypes-attribute
    #[ignore_malloc_size_of = "mozjs"]
    frozen_supported_performance_entry_types: CachedFrozenArray,

    /// The stack of active group labels for the Console APIs.
    console_group_stack: DomRefCell<Vec<DOMString>>,

    /// The count map for the Console APIs.
    ///
    /// <https://console.spec.whatwg.org/#count>
    console_count_map: DomRefCell<HashMap<DOMString, usize>>,

    // /// List of ongoing dynamic module imports.
    // dynamic_modules: DomRefCell<DynamicModuleList>,
    // 
    // /// Is considered in a secure context
    // inherited_secure_context: Option<bool>,
    // 
    // /// The byte length queuing strategy size function that will be initialized once
    // /// `size` getter of `ByteLengthQueuingStrategy` is called.
    // ///
    // /// <https://streams.spec.whatwg.org/#byte-length-queuing-strategy-size-function>
    // #[ignore_malloc_size_of = "Rc<T> is hard"]
    // byte_length_queuing_strategy_size_function: OnceCell<Rc<Function>>,
    // 
    // /// The count queuing strategy size function that will be initialized once
    // /// `size` getter of `CountQueuingStrategy` is called.
    // ///
    // /// <https://streams.spec.whatwg.org/#count-queuing-strategy-size-function>
    // #[ignore_malloc_size_of = "Rc<T> is hard"]
    // count_queuing_strategy_size_function: OnceCell<Rc<Function>>,

    // /// An import map allows control over module specifier resolution.
    // /// For now, only Window global objects have their import map modified from the initial empty one.
    // ///
    // /// <https://html.spec.whatwg.org/multipage/#import-maps>
    // import_map: DomRefCell<ImportMap>,
    // 
    // /// <https://html.spec.whatwg.org/multipage/#resolved-module-set>
    // resolved_module_set: DomRefCell<HashSet<ResolvedModule>>,
}

// /// Callback used to enqueue file chunks to streams as part of FileListener.
// fn stream_handle_incoming(stream: &ReadableStream, bytes: Fallible<Vec<u8>>, can_gc: CanGc) {
//     match bytes {
//         Ok(b) => {
//             stream.enqueue_native(b, can_gc);
//         },
//         Err(e) => {
//             stream.error_native(e, can_gc);
//         },
//     }
// }
//
// /// Callback used to close streams as part of FileListener.
// fn stream_handle_eof(stream: &ReadableStream, can_gc: CanGc) {
//     stream.controller_close_native(can_gc);
// }

impl GlobalScope {

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_inherited(
        pipeline_id: PipelineId,
        origin: MutableOrigin,
        creation_url: ServoUrl,
        top_level_creation_url: Option<ServoUrl>,
        microtask_queue: Rc<MicrotaskQueue>,
        // inherited_secure_context: Option<bool>,
    ) -> Self {
        Self {
            reflector: Reflector::new(),
            // task_manager: Default::default(),
            // blob_state: Default::default(),
            // eventtarget: EventTarget::new_inherited(),
            // crypto: Default::default(),
            pipeline_id,
            // console_timers: DomRefCell::new(Default::default()),
            // module_map: DomRefCell::new(Default::default()),
            // inline_module_map: DomRefCell::new(Default::default()),
            // in_error_reporting_mode: Default::default(),
            // timers: OnceCell::default(),
            origin,
            creation_url,
            top_level_creation_url,
            // permission_state_invocation_results: Default::default(),
            microtask_queue,
            // event_source_tracker: DOMTracker::new(),
            uncaught_rejections: Default::default(),
            consumed_rejections: Default::default(),
            frozen_supported_performance_entry_types: CachedFrozenArray::new(),
            console_group_stack: DomRefCell::new(Vec::new()),
            console_count_map: Default::default(),
            // dynamic_modules: DomRefCell::new(DynamicModuleList::new()),
            // byte_length_queuing_strategy_size_function: OnceCell::new(),
            // count_queuing_strategy_size_function: OnceCell::new(),
            // import_map: Default::default(),
            // resolved_module_set: Default::default(),
        }
    }
    /// Clean-up DOM related resources
    pub(crate) fn perform_a_dom_garbage_collection_checkpoint(&self) {
        // self.perform_a_blob_garbage_collection_checkpoint();
    }

    // pub(crate) fn track_event_source(&self, event_source: &EventSource) {
    //     self.event_source_tracker.track(event_source);
    // }
    //
    // pub(crate) fn close_event_sources(&self) -> bool {
    //     let mut canceled_any_fetch = false;
    //     self.event_source_tracker
    //         .for_each(
    //             |event_source: DomRoot<EventSource>| match event_source.ReadyState() {
    //                 2 => {},
    //                 _ => {
    //                     event_source.cancel();
    //                     canceled_any_fetch = true;
    //                 },
    //             },
    //         );
    //     canceled_any_fetch
    // }

    /// Returns the global scope of the realm that the given DOM object's reflector
    /// was created in.
    #[allow(unsafe_code)]
    pub(crate) fn from_reflector<T: DomObject>(reflector: &T, _realm: InRealm) -> DomRoot<Self> {
        unsafe { GlobalScope::from_object(*reflector.reflector().get_jsobject()) }
    }

    /// Returns the global scope of the realm that the given JS object was created in.
    #[allow(unsafe_code)]
    pub(crate) unsafe fn from_object(obj: *mut JSObject) -> DomRoot<Self> {
        assert!(!obj.is_null());
        let global = GetNonCCWObjectGlobal(obj);
        global_scope_from_global_static(global)
    }

    /// Returns the global scope for the given JSContext
    #[allow(unsafe_code)]
    pub(crate) unsafe fn from_context(cx: *mut JSContext, _realm: InRealm) -> DomRoot<Self> {
        let global = CurrentGlobalOrNull(cx);
        assert!(!global.is_null());
        global_scope_from_global(global, cx)
    }

    /// Returns the global scope for the given SafeJSContext
    #[allow(unsafe_code)]
    pub(crate) fn from_safe_context(cx: SafeJSContext, realm: InRealm) -> DomRoot<Self> {
        unsafe { Self::from_context(*cx, realm) }
    }

    /// Returns the global object of the realm that the given JS object
    /// was created in, after unwrapping any wrappers.
    #[allow(unsafe_code)]
    pub(crate) unsafe fn from_object_maybe_wrapped(
        mut obj: *mut JSObject,
        cx: *mut JSContext,
    ) -> DomRoot<Self> {
        if IsWrapper(obj) {
            obj = UnwrapObjectDynamic(obj, cx, /* stopAtWindowProxy = */ false);
            assert!(!obj.is_null());
        }
        GlobalScope::from_object(obj)
    }

    pub(crate) fn add_uncaught_rejection(&self, rejection: HandleObject) {
        self.uncaught_rejections
            .borrow_mut()
            .push(Heap::boxed(rejection.get()));
    }

    pub(crate) fn remove_uncaught_rejection(&self, rejection: HandleObject) {
        let mut uncaught_rejections = self.uncaught_rejections.borrow_mut();

        if let Some(index) = uncaught_rejections
            .iter()
            .position(|promise| *promise == Heap::boxed(rejection.get()))
        {
            uncaught_rejections.remove(index);
        }
    }

    // `Heap` values must stay boxed, as they need semantics like `Pin`
    // (that is, they cannot be moved).
    #[allow(clippy::vec_box)]
    pub(crate) fn get_uncaught_rejections(&self) -> &DomRefCell<Vec<Box<Heap<*mut JSObject>>>> {
        &self.uncaught_rejections
    }

    pub(crate) fn add_consumed_rejection(&self, rejection: HandleObject) {
        self.consumed_rejections
            .borrow_mut()
            .push(Heap::boxed(rejection.get()));
    }

    pub(crate) fn remove_consumed_rejection(&self, rejection: HandleObject) {
        let mut consumed_rejections = self.consumed_rejections.borrow_mut();

        if let Some(index) = consumed_rejections
            .iter()
            .position(|promise| *promise == Heap::boxed(rejection.get()))
        {
            consumed_rejections.remove(index);
        }
    }

    // `Heap` values must stay boxed, as they need semantics like `Pin`
    // (that is, they cannot be moved).
    #[allow(clippy::vec_box)]
    pub(crate) fn get_consumed_rejections(&self) -> &DomRefCell<Vec<Box<Heap<*mut JSObject>>>> {
        &self.consumed_rejections
    }

    // pub(crate) fn set_module_map(&self, url: ServoUrl, module: ModuleTree) {
    //     self.module_map.borrow_mut().insert(url, Rc::new(module));
    // }
    //
    // pub(crate) fn get_module_map(
    //     &self,
    // ) -> &DomRefCell<HashMapTracedValues<ServoUrl, Rc<ModuleTree>>> {
    //     &self.module_map
    // }
    //
    // pub(crate) fn set_inline_module_map(&self, script_id: ScriptId, module: ModuleTree) {
    //     self.inline_module_map
    //         .borrow_mut()
    //         .insert(script_id, Rc::new(module));
    // }
    //
    // pub(crate) fn get_inline_module_map(&self) -> &DomRefCell<HashMap<ScriptId, Rc<ModuleTree>>> {
    //     &self.inline_module_map
    // }

    #[allow(unsafe_code)]
    pub(crate) fn get_cx() -> SafeJSContext {
        let cx = Runtime::get()
            .expect("Can't obtain context after runtime shutdown")
            .as_ptr();
        unsafe { SafeJSContext::from_ptr(cx) }
    }

    // pub(crate) fn crypto(&self, can_gc: CanGc) -> DomRoot<Crypto> {
    //     self.crypto.or_init(|| Crypto::new(self, can_gc))
    // }

    // pub(crate) fn time(&self, label: DOMString) -> Result<(), ()> {
    //     let mut timers = self.console_timers.borrow_mut();
    //     if timers.len() >= 10000 {
    //         return Err(());
    //     }
    //     match timers.entry(label) {
    //         Entry::Vacant(entry) => {
    //             entry.insert(Instant::now());
    //             Ok(())
    //         },
    //         Entry::Occupied(_) => Err(()),
    //     }
    // }
    //
    // /// Computes the delta time since a label has been created
    // ///
    // /// Returns an error if the label does not exist.
    // pub(crate) fn time_log(&self, label: &str) -> Result<u64, ()> {
    //     self.console_timers
    //         .borrow()
    //         .get(label)
    //         .ok_or(())
    //         .map(|&start| (Instant::now() - start).as_millis() as u64)
    // }
    //
    // /// Computes the delta time since a label has been created and stops
    // /// tracking the label.
    // ///
    // /// Returns an error if the label does not exist.
    // pub(crate) fn time_end(&self, label: &str) -> Result<u64, ()> {
    //     self.console_timers
    //         .borrow_mut()
    //         .remove(label)
    //         .ok_or(())
    //         .map(|start| (Instant::now() - start).as_millis() as u64)
    // }

    /// Get the `PipelineId` for this global scope.
    pub(crate) fn pipeline_id(&self) -> PipelineId {
        self.pipeline_id
    }

    /// Get the origin for this global scope
    pub(crate) fn origin(&self) -> &MutableOrigin {
        &self.origin
    }

    /// Get the creation_url for this global scope
    pub(crate) fn creation_url(&self) -> &ServoUrl {
        &self.creation_url
    }

    /// Get the top_level_creation_url for this global scope
    pub(crate) fn top_level_creation_url(&self) -> &Option<ServoUrl> {
        &self.top_level_creation_url
    }

    // /// Schedule a [`TimerEventRequest`] on this [`GlobalScope`]'s [`timers::TimerScheduler`].
    // /// Every Worker has its own scheduler, which handles events in the Worker event loop,
    // /// but `Window`s use a shared scheduler associated with their [`ScriptThread`].
    // pub(crate) fn schedule_timer(&self, request: TimerEventRequest) -> Option<TimerId> {
    //     match self.downcast::<WorkerGlobalScope>() {
    //         Some(worker_global) => Some(worker_global.timer_scheduler().schedule_timer(request)),
    //         _ => unreachable!("There are only workers, and workers are all"),
    //     }
    // }

    /// Get the [base url](https://html.spec.whatwg.org/multipage/#api-base-url)
    /// for this global scope.
    pub(crate) fn api_base_url(&self) -> ServoUrl {
        if let Some(worker) = self.downcast::<StarlingGlobalScope>() {
            // https://html.spec.whatwg.org/multipage/#script-settings-for-workers:api-base-url
            return worker.get_url().clone();
        }
        // if let Some(_debugger_global) = self.downcast::<DebuggerGlobalScope>() {
        //     return self.creation_url.clone();
        // }
        unreachable!();
    }

    /// Get the URL for this global scope.
    pub(crate) fn get_url(&self) -> ServoUrl {
        if let Some(worker) = self.downcast::<StarlingGlobalScope>() {
            return worker.get_url().clone();
        }
        // if let Some(_debugger_global) = self.downcast::<DebuggerGlobalScope>() {
        //     return self.creation_url.clone();
        // }
        unreachable!();
    }

    // /// <https://html.spec.whatwg.org/multipage/#report-the-error>
    // pub(crate) fn report_an_error(&self, error_info: ErrorInfo, value: HandleValue, can_gc: CanGc) {
    //     // Step 6. Early return if global is in error reporting mode,
    //     if self.in_error_reporting_mode.get() {
    //         return;
    //     }
    //
    //     // Step 6.1. Set global's in error reporting mode to true.
    //     self.in_error_reporting_mode.set(true);
    //
    //     // Step 6.2. Set notHandled to the result of firing an event named error at global,
    //     // using ErrorEvent, with the cancelable attribute initialized to true,
    //     // and additional attributes initialized according to errorInfo.
    //
    //     // FIXME(#13195): muted errors.
    //     let event = ErrorEvent::new(
    //         self,
    //         atom!("error"),
    //         EventBubbles::DoesNotBubble,
    //         EventCancelable::Cancelable,
    //         error_info.message.as_str().into(),
    //         error_info.filename.as_str().into(),
    //         error_info.lineno,
    //         error_info.column,
    //         value,
    //         can_gc,
    //     );
    //
    //     let not_handled = event
    //         .upcast::<Event>()
    //         .fire(self.upcast::<EventTarget>(), can_gc);
    //
    //     // Step 6.3. Set global's in error reporting mode to false.
    //     self.in_error_reporting_mode.set(false);
    //
    //     // Step 7.
    //     if not_handled {
    //         // https://html.spec.whatwg.org/multipage/#runtime-script-errors-2
    //         if let Some(dedicated) = self.downcast::<DedicatedWorkerGlobalScope>() {
    //             dedicated.forward_error_to_worker_object(error_info);
    //         } else {
    //             unreachable!("There are only workers, and workers are all");
    //         }
    //     }
    // }

    // /// A sender to the event loop of this global scope. This either sends to the Worker event loop
    // /// or the ScriptThread event loop in the case of a `Window`. This can be `None` for dedicated
    // /// workers that are not currently handling a message.
    // pub(crate) fn event_loop_sender(&self) -> Option<ScriptEventLoopSender> {
    //     if let Some(dedicated) = self.downcast::<DedicatedWorkerGlobalScope>() {
    //         dedicated.event_loop_sender()
    //     } else {
    //         unreachable!("There are only workers, and workers are all");
    //     }
    // }

    // /// A reference to the [`TaskManager`] used to schedule tasks for this [`GlobalScope`].
    // pub(crate) fn task_manager(&self) -> &TaskManager {
    //     let shared_canceller = self
    //         .downcast::<WorkerGlobalScope>()
    //         .map(WorkerGlobalScope::shared_task_canceller);
    //     self.task_manager.get_or_init(|| {
    //         TaskManager::new(
    //             self.event_loop_sender(),
    //             self.pipeline_id(),
    //             shared_canceller,
    //         )
    //     })
    // }

    /// Evaluate JS code on this global scope.
    pub(crate) fn evaluate_js_on_global_with_result(
        &self,
        code: &str,
        rval: MutableHandleValue,
        // fetch_options: ScriptFetchOptions,
        script_base_url: ServoUrl,
        can_gc: CanGc,
        introduction_type: Option<&'static CStr>,
    ) -> bool {
        let source_code = SourceCode::Text(Rc::new(DOMString::from_string((*code).to_string())));
        self.evaluate_script_on_global_with_result(
            &source_code,
            "",
            rval,
            1,
            // fetch_options,
            script_base_url,
            can_gc,
            introduction_type,
        )
    }

    /// Evaluate a JS script on this global scope.
    #[allow(unsafe_code)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn evaluate_script_on_global_with_result(
        &self,
        code: &SourceCode,
        filename: &str,
        rval: MutableHandleValue,
        line_number: u32,
        // fetch_options: ScriptFetchOptions,
        script_base_url: ServoUrl,
        can_gc: CanGc,
        introduction_type: Option<&'static CStr>,
    ) -> bool {
        let cx = GlobalScope::get_cx();

        let ar = enter_realm(self);

        let _aes = AutoEntryScript::new(self);

        unsafe {
            rooted!(in(*cx) let mut compiled_script = std::ptr::null_mut::<JSScript>());
            match code {
                SourceCode::Text(text_code) => {
                    let mut options = CompileOptionsWrapper::new(*cx, filename, line_number);
                    if let Some(introduction_type) = introduction_type {
                        options.set_introduction_type(introduction_type);
                    }

                    debug!("compiling dom string");
                    compiled_script.set(Compile1(
                        *cx,
                        options.ptr,
                        &mut transform_str_to_source_text(text_code),
                    ));

                    if compiled_script.is_null() {
                        debug!("error compiling Dom string");
                        report_pending_exception(cx, true, InRealm::Entered(&ar), can_gc);
                        return false;
                    }
                },
                // SourceCode::Compiled(pre_compiled_script) => {
                //     let options = InstantiateOptions {
                //         skipFilenameValidation: false,
                //         hideScriptFromDebugger: false,
                //         deferDebugMetadata: false,
                //         eagerDelazificationStrategy_: DelazificationOption::OnDemandOnly,
                //     };
                //     let script = InstantiateGlobalStencil(
                //         *cx,
                //         &options,
                //         *pre_compiled_script.source_code,
                //         ptr::null_mut(),
                //     );
                //     compiled_script.set(script);
                // },
            };

            assert!(!compiled_script.is_null());

            rooted!(in(*cx) let mut script_private = UndefinedValue());
            JS_GetScriptPrivate(*compiled_script, script_private.handle_mut());

            // When `ScriptPrivate` for the compiled script is undefined,
            // we need to set it so that it can be used in dynamic import context.
            if script_private.is_undefined() {
                debug!("Set script private for {}", script_base_url);

                // let module_script_data = Rc::new(ModuleScript::new(
                //     script_base_url,
                //     fetch_options,
                //     // We can't initialize an module owner here because
                //     // the executing context of script might be different
                //     // from the dynamic import script's executing context.
                //     None,
                // ));
                //
                // SetScriptPrivate(
                //     *compiled_script,
                //     &PrivateValue(Rc::into_raw(module_script_data) as *const _),
                // );
            }

            let result = JS_ExecuteScript(*cx, compiled_script.handle(), rval);

            if !result {
                debug!("error evaluating Dom string");
                report_pending_exception(cx, true, InRealm::Entered(&ar), can_gc);
            }

            maybe_resume_unwind();
            result
        }
    }

    // /// <https://html.spec.whatwg.org/multipage/#timer-initialisation-steps>
    // pub(crate) fn schedule_callback(
    //     &self,
    //     callback: OneshotTimerCallback,
    //     duration: Duration,
    // ) -> OneshotTimerHandle {
    //     self.timers()
    //         .schedule_callback(callback, duration, self.timer_source())
    // }
    //
    // pub(crate) fn unschedule_callback(&self, handle: OneshotTimerHandle) {
    //     self.timers().unschedule_callback(handle);
    // }
    //
    // /// <https://html.spec.whatwg.org/multipage/#timer-initialisation-steps>
    // pub(crate) fn set_timeout_or_interval(
    //     &self,
    //     callback: TimerCallback,
    //     arguments: Vec<HandleValue>,
    //     timeout: Duration,
    //     is_interval: IsInterval,
    // ) -> i32 {
    //     self.timers().set_timeout_or_interval(
    //         self,
    //         callback,
    //         arguments,
    //         timeout,
    //         is_interval,
    //         self.timer_source(),
    //     )
    // }
    //
    // pub(crate) fn clear_timeout_or_interval(&self, handle: i32) {
    //     self.timers().clear_timeout_or_interval(self, handle);
    // }
    //
    pub(crate) fn queue_function_as_microtask(&self, callback: Rc<VoidFunction>) {
        self.enqueue_microtask(Microtask::User(UserMicrotask {
            callback,
            pipeline: self.pipeline_id(),
        }))
    }
    //
    // pub(crate) fn fire_timer(&self, handle: TimerEventId, can_gc: CanGc) {
    //     self.timers().fire_timer(handle, self, can_gc);
    // }
    //
    // pub(crate) fn resume(&self) {
    //     self.timers().resume();
    // }
    //
    // pub(crate) fn suspend(&self) {
    //     self.timers().suspend();
    // }
    //
    // pub(crate) fn slow_down_timers(&self) {
    //     self.timers().slow_down();
    // }
    //
    // pub(crate) fn speed_up_timers(&self) {
    //     self.timers().speed_up();
    // }
    //
    // fn timer_source(&self) -> TimerSource {
    //     if self.is::<Window>() {
    //         return TimerSource::FromWindow(self.pipeline_id());
    //     }
    //     if self.is::<WorkerGlobalScope>() {
    //         return TimerSource::FromWorker;
    //     }
    //     unreachable!();
    // }

    /// Returns a boolean indicating whether the event-loop
    /// where this global is running on can continue running JS.
    pub(crate) fn can_continue_running(&self) -> bool {
        // if self.is::<Window>() {
        //     return ScriptThread::can_continue_running();
        // }
        // if let Some(worker) = self.downcast::<WorkerGlobalScope>() {
        //     return !worker.is_closing();
        // }
        //
        // // TODO: plug worklets into this.
        true
    }

    /// Perform a microtask checkpoint.
    pub(crate) fn perform_a_microtask_checkpoint(&self, can_gc: CanGc) {
        // Only perform the checkpoint if we're not shutting down.
        if self.can_continue_running() {
            self.microtask_queue.checkpoint(
                GlobalScope::get_cx(),
                |_| Some(DomRoot::from_ref(self)),
                vec![DomRoot::from_ref(self)],
                can_gc,
            );
        }
    }

    /// Enqueue a microtask for subsequent execution.
    pub(crate) fn enqueue_microtask(&self, job: Microtask) {
        self.microtask_queue.enqueue(job, GlobalScope::get_cx());
    }

    /// Returns the microtask queue of this global.
    pub(crate) fn microtask_queue(&self) -> &Rc<MicrotaskQueue> {
        &self.microtask_queue
    }
    //
    // /// Process a single event as if it were the next event
    // /// in the queue for the event-loop where this global scope is running on.
    // /// Returns a boolean indicating whether further events should be processed.
    // pub(crate) fn process_event(&self, msg: CommonScriptMsg) -> bool {
    //     if self.is::<Window>() {
    //         return ScriptThread::process_event(msg);
    //     }
    //     if let Some(worker) = self.downcast::<WorkerGlobalScope>() {
    //         return worker.process_event(msg);
    //     }
    //     unreachable!();
    // }

    pub(crate) fn runtime_handle(&self) -> ParentRuntime {
        if let Some(worker) = self.downcast::<StarlingGlobalScope>() {
            worker.runtime_handle()
        } else {
            unreachable!()
        }
    }

    /// Returns the ["current"] global object.
    ///
    /// ["current"]: https://html.spec.whatwg.org/multipage/#current
    #[allow(unsafe_code)]
    pub(crate) fn current() -> Option<DomRoot<Self>> {
        let cx = Runtime::get()?;
        unsafe {
            let global = CurrentGlobalOrNull(cx.as_ptr());
            if global.is_null() {
                None
            } else {
                Some(global_scope_from_global(global, cx.as_ptr()))
            }
        }
    }

    /// Returns the ["entry"] global object.
    ///
    /// ["entry"]: https://html.spec.whatwg.org/multipage/#entry
    pub(crate) fn entry() -> DomRoot<Self> {
        entry_global()
    }

    /// Returns the ["incumbent"] global object.
    ///
    /// ["incumbent"]: https://html.spec.whatwg.org/multipage/#incumbent
    pub(crate) fn incumbent() -> Option<DomRoot<Self>> {
        incumbent_global()
    }

    // pub(crate) fn performance(&self) -> DomRoot<Performance> {
    //     if let Some(window) = self.downcast::<Window>() {
    //         return window.Performance();
    //     }
    //     if let Some(worker) = self.downcast::<WorkerGlobalScope>() {
    //         return worker.Performance();
    //     }
    //     unreachable!();
    // }
    //
    // /// <https://w3c.github.io/performance-timeline/#supportedentrytypes-attribute>
    // pub(crate) fn supported_performance_entry_types(
    //     &self,
    //     cx: SafeJSContext,
    //     retval: MutableHandleValue,
    //     can_gc: CanGc,
    // ) {
    //     self.frozen_supported_performance_entry_types.get_or_init(
    //         || {
    //             VALID_ENTRY_TYPES
    //                 .iter()
    //                 .map(|t| DOMString::from(t.to_string()))
    //                 .collect()
    //         },
    //         cx,
    //         retval,
    //         can_gc,
    //     );
    // }
    //
    // pub(crate) fn current_group_label(&self) -> Option<DOMString> {
    //     self.console_group_stack
    //         .borrow()
    //         .last()
    //         .map(|label| DOMString::from(format!("[{}]", label)))
    // }
    //
    // pub(crate) fn push_console_group(&self, group: DOMString) {
    //     self.console_group_stack.borrow_mut().push(group);
    // }
    //
    // pub(crate) fn pop_console_group(&self) {
    //     let _ = self.console_group_stack.borrow_mut().pop();
    // }
    //
    // pub(crate) fn increment_console_count(&self, label: &DOMString) -> usize {
    //     *self
    //         .console_count_map
    //         .borrow_mut()
    //         .entry(label.clone())
    //         .and_modify(|e| *e += 1)
    //         .or_insert(1)
    // }
    //
    // pub(crate) fn reset_console_count(&self, label: &DOMString) -> Result<(), ()> {
    //     match self.console_count_map.borrow_mut().get_mut(label) {
    //         Some(value) => {
    //             *value = 0;
    //             Ok(())
    //         },
    //         None => Err(()),
    //     }
    // }
    //
    // pub(crate) fn dynamic_module_list(&self) -> RefMut<DynamicModuleList> {
    //     self.dynamic_modules.borrow_mut()
    // }
    //
    // pub(crate) fn structured_clone(
    //     &self,
    //     cx: SafeJSContext,
    //     value: HandleValue,
    //     options: RootedTraceableBox<StructuredSerializeOptions>,
    //     retval: MutableHandleValue,
    // ) -> Fallible<()> {
    //     let mut rooted = CustomAutoRooter::new(
    //         options
    //             .transfer
    //             .iter()
    //             .map(|js: &RootedTraceableBox<Heap<*mut JSObject>>| js.get())
    //             .collect(),
    //     );
    //     let guard = CustomAutoRooterGuard::new(*cx, &mut rooted);
    //
    //     let data = structuredclone::write(cx, value, Some(guard))?;
    //
    //     structuredclone::read(self, data, retval)?;
    //
    //     Ok(())
    // }
    //
    // pub(crate) fn fetch<Listener: FetchResponseListener + PreInvoke + Send + 'static>(
    //     &self,
    //     request_builder: RequestBuilder,
    //     context: Arc<Mutex<Listener>>,
    //     task_source: SendableTaskSource,
    // ) {
    //     let network_listener = NetworkListener {
    //         context,
    //         task_source,
    //     };
    //     self.fetch_with_network_listener(request_builder, network_listener);
    // }
    //
    // pub(crate) fn fetch_with_network_listener<
    //     Listener: FetchResponseListener + PreInvoke + Send + 'static,
    // >(
    //     &self,
    //     request_builder: RequestBuilder,
    //     network_listener: NetworkListener<Listener>,
    // ) {
    //     fetch_async(
    //         &self.core_resource_thread(),
    //         request_builder,
    //         None,
    //         network_listener.into_callback(),
    //     );
    // }

    // pub(crate) fn set_byte_length_queuing_strategy_size(&self, function: Rc<Function>) {
    //     if self
    //         .byte_length_queuing_strategy_size_function
    //         .set(function)
    //         .is_err()
    //     {
    //         warn!("byte length queuing strategy size function is set twice.");
    //     };
    // }
    //
    // pub(crate) fn get_byte_length_queuing_strategy_size(&self) -> Option<Rc<Function>> {
    //     self.byte_length_queuing_strategy_size_function
    //         .get()
    //         .cloned()
    // }
    //
    // pub(crate) fn set_count_queuing_strategy_size(&self, function: Rc<Function>) {
    //     if self
    //         .count_queuing_strategy_size_function
    //         .set(function)
    //         .is_err()
    //     {
    //         warn!("count queuing strategy size function is set twice.");
    //     };
    // }
    //
    // pub(crate) fn get_count_queuing_strategy_size(&self) -> Option<Rc<Function>> {
    //     self.count_queuing_strategy_size_function.get().cloned()
    // }

    // pub(crate) fn import_map(&self) -> Ref<'_, ImportMap> {
    //     self.import_map.borrow()
    // }
    // 
    // pub(crate) fn import_map_mut(&self) -> RefMut<'_, ImportMap> {
    //     self.import_map.borrow_mut()
    // }
    // 
    // pub(crate) fn resolved_module_set(&self) -> Ref<'_, HashSet<ResolvedModule>> {
    //     self.resolved_module_set.borrow()
    // }
    // 
    // pub(crate) fn resolved_module_set_mut(&self) -> RefMut<'_, HashSet<ResolvedModule>> {
    //     self.resolved_module_set.borrow_mut()
    // }
    // 
    // /// <https://html.spec.whatwg.org/multipage/#add-module-to-resolved-module-set>
    // pub(crate) fn add_module_to_resolved_module_set(
    //     &self,
    //     base_url: &str,
    //     specifier: &str,
    //     specifier_url: Option<ServoUrl>,
    // ) {
    //     // Step 1. Let global be settingsObject's global object.
    //     // Step 2. If global does not implement Window, then return.
    //     if self.is::<Window>() {
    //         // Step 3. Let record be a new specifier resolution record, with serialized base URL
    //         // set to serializedBaseURL, specifier set to normalizedSpecifier, and specifier as
    //         // a URL set to asURL.
    //         let record =
    //             ResolvedModule::new(base_url.to_owned(), specifier.to_owned(), specifier_url);
    //         // Step 4. Append record to global's resolved module set.
    //         self.resolved_module_set.borrow_mut().insert(record);
    //     }
    // }
}

/// Returns the Rust global scope from a JS global object.
#[allow(unsafe_code)]
unsafe fn global_scope_from_global(
    global: *mut JSObject,
    cx: *mut JSContext,
) -> DomRoot<GlobalScope> {
    assert!(!global.is_null());
    let clasp = get_object_class(global);
    assert_ne!(
        ((*clasp).flags & (JSCLASS_IS_DOMJSCLASS | JSCLASS_IS_GLOBAL)),
        0
    );
    root_from_object(global, cx).unwrap()
}

/// Returns the Rust global scope from a JS global object.
#[allow(unsafe_code)]
unsafe fn global_scope_from_global_static(global: *mut JSObject) -> DomRoot<GlobalScope> {
    assert!(!global.is_null());
    let clasp = get_object_class(global);
    assert_ne!(
        ((*clasp).flags & (JSCLASS_IS_DOMJSCLASS | JSCLASS_IS_GLOBAL)),
        0
    );
    root_from_object_static(global).unwrap()
}

#[allow(unsafe_code)]
impl GlobalScopeHelpers<crate::DomTypeHolder> for GlobalScope {
    unsafe fn from_context(cx: *mut JSContext, realm: InRealm) -> DomRoot<Self> {
        GlobalScope::from_context(cx, realm)
    }

    fn get_cx() -> SafeJSContext {
        GlobalScope::get_cx()
    }

    unsafe fn from_object(obj: *mut JSObject) -> DomRoot<Self> {
        GlobalScope::from_object(obj)
    }

    fn from_reflector(reflector: &impl DomObject, realm: InRealm) -> DomRoot<Self> {
        GlobalScope::from_reflector(reflector, realm)
    }

    fn origin(&self) -> &MutableOrigin {
        GlobalScope::origin(self)
    }

    fn incumbent() -> Option<DomRoot<Self>> {
        GlobalScope::incumbent()
    }

    fn perform_a_microtask_checkpoint(&self, can_gc: CanGc) {
        GlobalScope::perform_a_microtask_checkpoint(self, can_gc)
    }

    fn get_url(&self) -> ServoUrl {
        self.get_url()
    }

    fn is_secure_context(&self) -> bool {
        unimplemented!()
    }
}
