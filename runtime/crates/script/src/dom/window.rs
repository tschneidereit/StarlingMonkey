/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::RefCell;

// use base::id::{Index, PipelineId, PipelineNamespaceId};
// use constellation_traits::ScriptToConstellationChan;
// use devtools_traits::{DevtoolScriptControlMsg, ScriptToDevtoolsControlMsg, SourceInfo, WorkerId};
use dom_struct::dom_struct;
// use embedder_traits::resources::{self, Resource};
// use embedder_traits::{JavaScriptEvaluationError, ScriptToEmbedderChan};
// use ipc_channel::ipc::IpcSender;
// use js::jsval::UndefinedValue;
// use js::rust::wrappers::JS_DefineDebuggerObject;
// use net_traits::ResourceThreads;
// use profile_traits::{mem, time};
// use script_bindings::codegen::GenericBindings::DebuggerGetPossibleBreakpointsEventBinding::RecommendedBreakpointLocation;
// use script_bindings::codegen::GenericBindings::DebuggerGlobalScopeBinding::{
//     DebuggerGlobalScopeMethods, NotifyNewSource,
// };
use script_bindings::realms::InRealm;
use script_bindings::reflector::DomObject;
use servo_url::{ImmutableOrigin, MutableOrigin, ServoUrl};
// use storage_traits::StorageThreads;

use crate::dom::bindings::codegen::Bindings::WindowGlobalScopeBinding;
use crate::dom::bindings::error::report_pending_exception;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::utils::define_all_exposed_interfaces;
use crate::dom::globalscope::GlobalScope;
// use crate::dom::types::{DebuggerAddDebuggeeEvent, DebuggerGetPossibleBreakpointsEvent, Event};
#[cfg(feature = "testbinding")]
#[cfg(feature = "webgpu")]
use crate::dom::webgpu::identityhub::IdentityHub;
use crate::realms::enter_realm;
// use crate::script_module::ScriptFetchOptions;
use crate::script_runtime::{CanGc, IntroductionType, JSContext};

#[dom_struct]
/// Global scope for interacting with the devtools Debugger API.
///
/// <https://firefox-source-docs.mozilla.org/js/Debugger/>
pub(crate) struct WindowGlobalScope {
    global_scope: GlobalScope,
    // #[no_trace]
    // devtools_to_script_sender: IpcSender<DevtoolScriptControlMsg>,
    // #[no_trace]
    // get_possible_breakpoints_result_sender:
    //     RefCell<Option<IpcSender<Vec<devtools_traits::RecommendedBreakpointLocation>>>>,
}

impl WindowGlobalScope {
    /// Get the JS context.
    pub(crate) fn get_cx() -> JSContext {
        GlobalScope::get_cx()
    }

    pub(crate) fn as_global_scope(&self) -> &GlobalScope {
        self.upcast::<GlobalScope>()
    }
}
