//! Runtime entry points.
//!
//! Replaces `runtime/js.cpp`. Provides wizer pre-initialization, CLI run,
//! lazy init from environment, and clock offset handling for wizer resume.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::config::EngineConfig;
use crate::engine::Engine;

/// Monotonic clock offset — added to CLOCK_MONOTONIC readings to ensure
/// monotonicity across wizer snapshot resumptions.
static MONO_CLOCK_OFFSET: AtomicU64 = AtomicU64::new(0);


extern "C" {
    /// WASI clock API
    fn __wasi_clock_time_get(
        clock_id: u32,
        precision: u64,
        time: *mut u64,
    ) -> u16;

    /// Deinitialize wasi-libc's cached environment
    fn __wasilibc_deinitialize_environ();

    /// Set the C-side mono_clock_offset (in init.cpp) for clock_gettime override.
    fn starling_set_mono_clock_offset(offset: u64);
}

/// WASI clock IDs
const CLOCKID_MONOTONIC: u32 = 1;

/// Wizer pre-initialization entry point.
///
/// Reads args from stdin, creates and initializes the engine in
/// pre-initialization mode, then snapshots. Called by the `wizer-initialize`
/// export.
///
/// # Safety
/// Called from C++ wizer init macro.
#[no_mangle]
pub unsafe extern "C" fn starling_wizer_init() {
    // Read args from stdin (one line)
    let config = match EngineConfig::from_stdin() {
        Ok(c) => c,
        Err(_) => EngineConfig::default(),
    };

    // Force pre-initialization mode
    let mut config = config;
    config.pre_initialize = true;

    match Engine::new(config) {
        Ok(mut engine) => {
            engine.finish_pre_initialization();
            // Leak the engine — wizer will snapshot the memory
            let _ = Box::into_raw(engine);
        }
        Err(e) => {
            eprintln!("StarlingMonkey wizer init failed: {e}");
            std::process::exit(1);
        }
    }

    // Record the current monotonic time so we can ensure monotonicity
    // after wizer resume.
    update_mono_clock_offset();

    // Deinitialize cached environ so it gets re-read after wizer resume.
    __wasilibc_deinitialize_environ();
}

/// Initialize the runtime lazily from the environment.
///
/// Checks `STARLINGMONKEY_CONFIG` env var. Idempotent — no-op if already
/// initialized (e.g., by wizer).
///
/// # Safety
/// Called from C++ request handler and other entry points.
#[no_mangle]
pub unsafe extern "C" fn starling_init_from_environment() -> bool {
    if Engine::is_initialized() {
        return true;  // Already initialized (e.g., by wizer)
    }

    let config = match EngineConfig::from_env() {
        Ok(c) => c,
        Err(_) => EngineConfig::default(),
    };

    match Engine::new(config) {
        Ok(engine) => {
            // Leak the engine — it lives for the process lifetime
            let _ = Box::into_raw(engine);
            true
        }
        Err(e) => {
            eprintln!("StarlingMonkey init failed: {e}");
            false
        }
    }
}

/// Direct extern "C" symbol called from C++ init.cpp `init_from_environment()`.
/// This is the same as starling_init_from_environment but with the name the
/// C++ request_handler.cpp expects.
#[no_mangle]
pub unsafe extern "C" fn init_from_environment() -> bool {
    starling_init_from_environment()
}

/// CLI run entry point — called from host_api.cpp `starling_cli_run()`.
///
/// Parses WASI CLI arguments to configure the engine.
///
/// # Safety
/// Called from C++ `starling_cli_run` in host_api.cpp.
#[no_mangle]
pub unsafe extern "C" fn starling_cli_run_init() -> bool {
    if Engine::is_initialized() {
        return true;
    }

    let config = match EngineConfig::from_args(std::env::args()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("StarlingMonkey CLI config error: {e}");
            return false;
        }
    };

    match Engine::new(config) {
        Ok(engine) => {
            let _ = Box::into_raw(engine);
            true
        }
        Err(e) => {
            eprintln!("StarlingMonkey CLI init failed: {e}");
            false
        }
    }
}

/// Direct extern "C" symbol for the WASI cli/run export.
/// Called from host_api.cpp starling_cli_run → exports_wasi_cli_run_run chain.
/// This cuts out one hop: starling_cli_run can just call this directly.
#[no_mangle]
pub unsafe extern "C" fn starling_cli_run() -> bool {
    starling_cli_run_init()
}

/// Direct extern "C" symbol for the wasi:cli/run#run export.
/// Called by the component model runtime.
#[no_mangle]
pub unsafe extern "C" fn exports_wasi_cli_run_run() -> bool {
    starling_cli_run_init()
}

/// Get the monotonic clock offset.
///
/// This is added to `CLOCK_MONOTONIC` readings to maintain monotonicity
/// across wizer snapshot boundaries.
pub fn mono_clock_offset() -> u64 {
    MONO_CLOCK_OFFSET.load(Ordering::Relaxed)
}

/// Update the monotonic clock offset after wizer snapshot.
fn update_mono_clock_offset() {
    let mut t: u64 = 0;
    let err = unsafe { __wasi_clock_time_get(CLOCKID_MONOTONIC, 1, &mut t) };
    if err == 0 {
        let prev = MONO_CLOCK_OFFSET.load(Ordering::Relaxed);
        if t > prev {
            MONO_CLOCK_OFFSET.store(t, Ordering::Relaxed);
        }
        // Also update the C-side offset used by the clock_gettime override.
        unsafe { starling_set_mono_clock_offset(MONO_CLOCK_OFFSET.load(Ordering::Relaxed)) };
    }
}
