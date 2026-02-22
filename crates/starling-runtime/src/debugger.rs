//! Debugger support (feature-gated behind `debugger`).
//!
//! Replaces `runtime/debugger.cpp`. Uses the SpiderMonkey Debugger API
//! via sm_sys to create a debugger realm, connect to a TCP debugging port,
//! and evaluate a remotely-provided debugging script.

use starling_sm_sys as sm;

/// State for the JS debugger connection.
static mut DEBUGGER_INITIALIZED: bool = false;

/// Replacement script path set by the debugger.
static mut REPLACEMENT_PATH: Option<String> = None;

/// Try to initialize the debugger if debugging is enabled.
///
/// Checks for the `DEBUGGER_PORT` environment variable and connects to the
/// debugging server. If no port is set, debugging is silently skipped.
///
/// # Safety
/// Must be called with a valid engine context.
pub unsafe fn maybe_init_debugger(cx: *mut sm::JSContext, debugging_enabled: bool) {
    if DEBUGGER_INITIALIZED || !debugging_enabled {
        return;
    }
    DEBUGGER_INITIALIZED = true;

    // TODO: Implement TCP socket connection to debugger server
    // 1. Read DEBUGGER_PORT env var
    // 2. Connect to localhost:$DEBUGGER_PORT
    // 3. Send "get-session-port" and read response
    // 4. Connect to session port
    // 5. Send "get-debugger" and read debugging script
    // 6. Create invisible debugger global with Debugger API
    // 7. Evaluate the debugging script in the debugger global
    //
    // This requires TCP socket support from starling-host-api::sockets.
    // For now, this is a no-op stub.
    let _ = cx;
}

/// Get a replacement script path set by the debugger, if any.
pub fn replacement_script_path() -> Option<&'static str> {
    // Safety: only accessed from the main thread, after maybe_init_debugger
    unsafe { REPLACEMENT_PATH.as_deref() }
}
