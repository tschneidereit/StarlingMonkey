// Copyright 2015 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Routers allow converting IPC channels to crossbeam channels.
//! The [RouterProxy] provides various methods to register
//! `IpcReceiver<T>`s. The router will then either call the appropriate callback or route the
//! message to a crossbeam `Sender<T>` or `Receiver<T>`. You should use the global `ROUTER` to
//! access the `RouterProxy` methods (via `ROUTER`'s `Deref` for `RouterProxy`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crossbeam_channel::{self, Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::ipc::{IpcMessage, IpcReceiver, OpaqueIpcReceiver};

/// Global object wrapping a `RouterProxy`.
/// Add routes ([add_route](RouterProxy::add_route)), or convert IpcReceiver<T>
/// to crossbeam channels (e.g. [route_ipc_receiver_to_new_crossbeam_receiver](RouterProxy::route_ipc_receiver_to_new_crossbeam_receiver))
pub static ROUTER: LazyLock<RouterProxy> = LazyLock::new(RouterProxy::new);

/// A `RouterProxy` provides methods for talking to the router. In single-threaded mode,
/// the RouterProxy registers callbacks directly on IpcReceivers without spawning a thread.
pub struct RouterProxy {
    next_handler_id: Mutex<u64>,
    // Keep receivers alive so they don't get dropped
    receivers: Mutex<HashMap<u64, OpaqueIpcReceiver>>,
}

#[allow(clippy::new_without_default)]
impl RouterProxy {
    pub fn new() -> RouterProxy {
        RouterProxy {
            next_handler_id: Mutex::new(0),
            receivers: Mutex::new(HashMap::new()),
        }
    }

    /// Add a new (receiver, callback) pair to the router.
    ///
    /// Consider using [add_typed_route](Self::add_typed_route) instead, which prevents
    /// mismatches between the receiver and callback types.
    #[deprecated(since = "0.19.0", note = "please use 'add_typed_route' instead")]
    pub fn add_route(&self, receiver: OpaqueIpcReceiver, mut callback: RouterHandler) {
        // In single-threaded mode, register the callback directly on the receiver
        receiver.register_callback(move |msg| callback(msg));

        // Allocate handler ID and store the receiver to keep it alive
        let handler_id = {
            let mut next_id = self.next_handler_id.lock().unwrap();
            let id = *next_id;
            *next_id += 1;
            id
        };

        self.receivers.lock().unwrap().insert(handler_id, receiver);
    }

    /// Add a new `(receiver, callback)` pair to the router.
    ///
    /// Unlike [add_route](Self::add_route) this method is strongly typed and guarantees
    /// that the `receiver` and the `callback` use the same message type.
    pub fn add_typed_route<T>(&self, receiver: IpcReceiver<T>, mut callback: TypedRouterHandler<T>)
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        // Before passing the message on to the callback, turn it into the appropriate type
        let modified_callback = move |msg: IpcMessage| {
            let typed_message = msg.to::<T>();
            callback(typed_message)
        };

        #[allow(deprecated)]
        self.add_route(receiver.to_opaque(), Box::new(modified_callback));
    }

    /// Shutdown is a no-op in single-threaded mode
    /// Calling it is idempotent,
    /// which can be useful when running a multi-process system in single-process mode.
    pub fn shutdown(&self) {
        // No-op in single-threaded mode
    }

    /// A convenience function to route an `IpcReceiver<T>` to an existing `Sender<T>`.
    pub fn route_ipc_receiver_to_crossbeam_sender<T>(
        &self,
        ipc_receiver: IpcReceiver<T>,
        crossbeam_sender: Sender<T>,
    ) where
        T: for<'de> Deserialize<'de> + Serialize + Send + 'static,
    {
        self.add_typed_route(
            ipc_receiver,
            Box::new(move |message| drop(crossbeam_sender.send(message.unwrap()))),
        )
    }

    /// A convenience function to route an `IpcReceiver<T>` to a `Receiver<T>`: the most common
    /// use of a `Router`.
    pub fn route_ipc_receiver_to_new_crossbeam_receiver<T>(
        &self,
        ipc_receiver: IpcReceiver<T>,
    ) -> Receiver<T>
    where
        T: for<'de> Deserialize<'de> + Serialize + Send + 'static,
    {
        let (crossbeam_sender, crossbeam_receiver) = crossbeam_channel::unbounded();
        self.route_ipc_receiver_to_crossbeam_sender(ipc_receiver, crossbeam_sender);
        crossbeam_receiver
    }
}

/// Function to call when a new event is received from the corresponding receiver.
pub type RouterHandler = Box<dyn FnMut(IpcMessage) + Send>;

/// Like [RouterHandler] but includes the type that will be passed to the callback
pub type TypedRouterHandler<T> = Box<dyn FnMut(Result<T, bincode::Error>) + Send>;
