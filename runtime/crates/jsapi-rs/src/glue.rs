//! Rust wrappers for mozjs's glue module

// use crate::jsapi::{ForwardingProxyHandler, WrapperProxyHandler, JobQueueTraps, ProxyTraps};
use core::mem;

pub use crate::jsapi::jsglue::*;

pub type EncodedStringCallback = unsafe extern "C" fn(*const core::ffi::c_char);

// manual glue stuff
unsafe impl Sync for ProxyTraps {}

impl Default for JobQueueTraps {
    fn default() -> JobQueueTraps {
        unsafe { mem::zeroed() }
    }
}

impl Default for ProxyTraps {
    fn default() -> ProxyTraps {
        unsafe { mem::zeroed() }
    }
}

impl Default for WrapperProxyHandler {
    fn default() -> WrapperProxyHandler {
        unsafe { mem::zeroed() }
    }
}

impl Default for ForwardingProxyHandler {
    fn default() -> ForwardingProxyHandler {
        unsafe { mem::zeroed() }
    }
}
