/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#![allow(unused_imports)]
#![allow(dead_code)]

#![cfg_attr(crown, feature(register_tool))]
#![deny(unsafe_code)]
#![doc = "The script crate contains all matters DOM."]
// Register the linter `crown`, which is the Servo-specific linter for the script crate.
#![cfg_attr(crown, register_tool(crown))]

// These are used a lot so let's keep them for now
#[macro_use]
extern crate js;
#[macro_use]
extern crate jstraceable_derive;
#[macro_use]
extern crate log;
#[macro_use]
extern crate malloc_size_of_derive;
#[macro_use]
extern crate stylo_atoms;

#[macro_use]
mod task;
mod body;
pub(crate) mod conversions;
// mod devtools;
#[macro_use]
mod dom;
pub(crate) mod fetch;
// pub(crate) mod indexed_db;
mod init;
#[allow(unsafe_code)]
pub(crate) mod messaging;
mod microtask;
pub(crate) mod mime;
// mod navigation;
mod network_listener;
mod realms;
// mod routed_promise;
// #[allow(dead_code)]
// mod script_module;
pub(crate) mod script_runtime;
mod task_manager;
mod task_queue;
mod task_source;
mod timers;

pub use init::init;
pub(crate) use script_bindings::DomTypes;
pub use script_runtime::JSEngineSetup;

pub(crate) use crate::dom::bindings::codegen::DomTypeHolder::DomTypeHolder;
// These trait exports are public, because they are used in the DOM bindings.
// Since they are used in derive macros,
// it is useful that they are accessible at the root of the crate.
pub(crate) use crate::dom::bindings::inheritance::HasParent;
pub(crate) use crate::dom::bindings::reflector::{DomObject, MutDomObject, Reflector};
pub(crate) use crate::dom::bindings::trace::{CustomTraceable, JSTraceable};

pub use script_runtime::CanGc;
pub use dom::starlingglobalscope::StarlingGlobalScope as GlobalScope;
