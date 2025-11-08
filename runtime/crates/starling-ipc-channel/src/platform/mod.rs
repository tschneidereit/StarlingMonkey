// Copyright 2015 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[cfg(all(
    not(any(feature = "force-inprocess", feature = "single-thread")),
    any(
        target_os = "linux",
        target_os = "openbsd",
        target_os = "freebsd",
        target_os = "illumos",
    )
))]
mod unix;
#[cfg(all(
    not(any(feature = "force-inprocess", feature = "single-thread")),
    any(
        target_os = "linux",
        target_os = "openbsd",
        target_os = "freebsd",
        target_os = "illumos",
    )
))]
mod os {
    pub use super::unix::*;
}

#[cfg(all(not(any(feature = "force-inprocess", feature = "single-thread")), target_os = "macos"))]
mod macos;
#[cfg(all(not(any(feature = "force-inprocess", feature = "single-thread")), target_os = "macos"))]
mod os {
    pub use super::macos::*;
}

#[cfg(all(not(any(feature = "force-inprocess", feature = "single-thread")), target_os = "windows"))]
mod windows;
#[cfg(all(not(any(feature = "force-inprocess", feature = "single-thread")), target_os = "windows"))]
mod os {
    pub use super::windows::*;
}

#[cfg(all(any(
    feature = "force-inprocess",
    target_os = "android",
    target_os = "ios",
    target_os = "wasi",
    target_os = "unknown"),
    not(feature = "single-thread")
))]
mod inprocess;
#[cfg(all(any(
    feature = "force-inprocess",
    target_os = "android",
    target_os = "ios",
    target_os = "wasi",
    target_os = "unknown"),
    not(feature = "single-thread")
))]
mod os {
    pub use super::inprocess::*;
}

#[cfg(feature = "single-thread")]
mod single_thread;

#[cfg(feature = "single-thread")]
mod os {
    pub use super::single_thread::*;
}

pub use self::os::{channel, OsOpaqueIpcChannel};
pub use self::os::{OsIpcChannel, OsIpcOneShotServer, OsIpcReceiver, OsIpcReceiverSet};
pub use self::os::{OsIpcSelectionResult, OsIpcSender, OsIpcSharedMemory};

#[cfg(test)]
mod test;
