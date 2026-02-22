//! Runtime entry points.
//!
//! Replaces `runtime/js.cpp`. Provides wizer pre-initialization, CLI run,
//! lazy init from environment, and clock offset handling for wizer resume.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::config::EngineConfig;
use crate::engine::Engine;

/// Monotonic clock offset — added to CLOCK_MONOTONIC readings to ensure
/// monotonicity across wizer snapshot resumptions.
static MONO_CLOCK_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Whether the engine has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

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
            INITIALIZED.store(true, Ordering::SeqCst);
        }
        Err(e) => {
            eprintln!("StarlingMonkey wizer init failed: {e}");
        }
    }

    // Record the current monotonic time so we can ensure monotonicity
    // after wizer resume.
    update_mono_clock_offset();
}

/// Initialize the runtime lazily from the environment.
///
/// Checks `STARLINGMONKEY_CONFIG` env var. Idempotent — no-op if already
/// initialized (e.g., by wizer).
///
/// # Safety
/// Called from C++ request handler.
#[no_mangle]
pub unsafe extern "C" fn starling_init_from_environment() -> bool {
    if INITIALIZED.load(Ordering::SeqCst) {
        return true;
    }

    let config = match EngineConfig::from_env() {
        Ok(c) => c,
        Err(_) => EngineConfig::default(),
    };

    match Engine::new(config) {
        Ok(engine) => {
            // Leak the engine — it lives for the process lifetime
            let _ = Box::into_raw(engine);
            INITIALIZED.store(true, Ordering::SeqCst);
            true
        }
        Err(e) => {
            eprintln!("StarlingMonkey init failed: {e}");
            false
        }
    }
}

/// CLI run entry point.
///
/// Parses WASI CLI arguments to configure the engine.
///
/// # Safety
/// Called from C++ `exports_wasi_cli_run_run`.
#[no_mangle]
pub unsafe extern "C" fn starling_cli_run_init() -> bool {
    if INITIALIZED.load(Ordering::SeqCst) {
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
            INITIALIZED.store(true, Ordering::SeqCst);
            true
        }
        Err(e) => {
            eprintln!("StarlingMonkey CLI init failed: {e}");
            false
        }
    }
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
    // Read current monotonic time via WASI
    // In the final build, this calls the WASI clock_time_get import.
    // For now, store 0 — the actual WASI call will be wired up when
    // the full build is functional.
    // TODO: Wire up wasi:clocks/monotonic-clock.now() call
    let _current = 0u64;
    let prev = MONO_CLOCK_OFFSET.load(Ordering::Relaxed);
    if _current > prev {
        MONO_CLOCK_OFFSET.store(_current, Ordering::Relaxed);
    }
}
