/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

#![crate_name = "spidermonkey_rs"]
#![crate_type = "rlib"]
#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    improper_ctypes
)]

//!
//! This crate contains Rust bindings to the [SpiderMonkey Javascript engine][1]
//! developed by Mozilla.
//!
//! These bindings are designed to be a fairly straightforward translation to the C++ API, while
//! taking advantage of Rust's memory safety. For more about the Spidermonkey API, see the
//! [API Reference][2] and the [User Guide][3] on MDN, and the [embedding examples][4] on GitHub.
//!
//! The code from User Guide sections [A minimal example](https://github.com/servo/mozjs/blob/main/mozjs/examples/minimal.rs) and
//! [Running scripts](https://github.com/servo/mozjs/blob/main/mozjs/examples/eval.rs) are also included.
//!
//! [1]: https://developer.mozilla.org/en-US/docs/Mozilla/Projects/SpiderMonkey
//! [2]: https://developer.mozilla.org/en-US/docs/Mozilla/Projects/SpiderMonkey/JSAPI_reference
//! [3]: https://developer.mozilla.org/en-US/docs/Mozilla/Projects/SpiderMonkey/JSAPI_User_Guide
//! [4]: https://github.com/mozilla-spidermonkey/spidermonkey-embedding-examples/
//!

pub mod raw {
    pub use jsapi_rs::jsapi::*;
}

pub mod jsapi {
    // Resolve ambiguous imports
    pub use jsapi_rs::jsapi::js::detail;
    pub use jsapi_rs::jsapi::JS::{FrontendContext, MemoryUse};

    pub use jsapi_rs::jsapi::js::detail::*;
    pub use jsapi_rs::jsapi::js::*;
    pub use jsapi_rs::jsapi::js::ForwardingProxyHandler;
    pub use jsapi_rs::jsapi::mozilla::MallocSizeOf;
    pub use jsapi_rs::jsapi::JS::detail::*;
    pub use jsapi_rs::jsapi::JS::shadow::Object;
    pub use jsapi_rs::jsapi::JS::Scalar::Type;
    pub use jsapi_rs::jsapi::JS::*;
    pub use jsapi_rs::jsapi::*;
    pub use jsapi_rs::jsapi::NewProxyObject;
    pub mod glue {
        pub use super::jsglue::*;
    }
}

pub(crate) mod js {
    pub mod glue {
        // pub use jsapi_rs::jsapi::jsglue::*;
    }

    // pub use crate::conversions;
    // pub use crate::error;
    // pub use crate::rust;
    // pub use crate::jsval;
    // pub use crate::jsid;
    // pub use crate::gc;
    // pub use crate::rooted;
    // pub use crate::typedarray;
    // pub use crate::consts::*;


    pub mod jsapi {
        // // Resolve ambiguous imports
        // pub use jsapi_rs::jsapi::js::detail;
        // pub use jsapi_rs::jsapi::JS::{FrontendContext, MemoryUse};
        // 
        // pub use jsapi_rs::jsapi::jsglue::ForwardingProxyHandler;
        // pub use jsapi_rs::jsapi::jsglue::*;
        // pub use jsapi_rs::jsapi::js::detail::*;
        // pub use jsapi_rs::jsapi::js::*;
        // pub use jsapi_rs::jsapi::mozilla::MallocSizeOf;
        // pub use jsapi_rs::jsapi::JS::detail::*;
        // pub use jsapi_rs::jsapi::JS::shadow::Object;
        // pub use jsapi_rs::jsapi::JS::Scalar::Type;
        // pub use jsapi_rs::jsapi::JS::*;
        // pub use jsapi_rs::jsapi::*;
    }
}

#[macro_use]
pub mod rust;

pub mod consts;
pub mod conversions;
pub mod error;
pub mod gc;
pub mod panic;
pub mod typedarray;

pub use crate::consts::*;
pub use jsapi_rs::glue;
pub use jsapi_rs::jsgc;
pub use jsapi_rs::jsid;
pub use jsapi_rs::jsval;

pub use crate::jsval::JS_ARGV;
pub use crate::jsval::JS_CALLEE;
