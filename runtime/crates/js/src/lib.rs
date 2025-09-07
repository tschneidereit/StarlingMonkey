pub mod glue {
    pub use jsapi_rs::jsapi::jsglue::*;
}

pub use spidermonkey_rs::conversions;
pub use spidermonkey_rs::error;
pub use spidermonkey_rs::rust;
pub use spidermonkey_rs::jsval;
pub use spidermonkey_rs::jsid;
pub use spidermonkey_rs::gc;
pub use spidermonkey_rs::rooted;
pub use spidermonkey_rs::typedarray;
pub use spidermonkey_rs::consts::*;


pub mod jsapi {
    // Resolve ambiguous imports
    pub use jsapi_rs::jsapi::js::detail;
    pub use jsapi_rs::jsapi::JS::{FrontendContext, MemoryUse};

    pub use jsapi_rs::jsapi::jsglue::ForwardingProxyHandler;
    pub use jsapi_rs::jsapi::jsglue::*;
    pub use jsapi_rs::jsapi::js::detail::*;
    pub use jsapi_rs::jsapi::js::*;
    pub use jsapi_rs::jsapi::mozilla::MallocSizeOf;
    pub use jsapi_rs::jsapi::JS::detail::*;
    pub use jsapi_rs::jsapi::JS::shadow::Object;
    pub use jsapi_rs::jsapi::JS::Scalar::Type;
    pub use jsapi_rs::jsapi::JS::*;
    pub use jsapi_rs::jsapi::*;
}

pub use crate::jsval::JS_ARGV;
pub use crate::jsval::JS_CALLEE;
