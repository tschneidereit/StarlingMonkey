use wasi::clocks::monotonic_clock;
use wasi::io::poll::Pollable;

/// Get the current monotonic clock time in nanoseconds.
#[no_mangle]
pub extern "C" fn host_api_monotonic_clock_now() -> u64 {
    monotonic_clock::now()
}

/// Get the monotonic clock resolution in nanoseconds.
#[no_mangle]
pub extern "C" fn host_api_monotonic_clock_resolution() -> u64 {
    monotonic_clock::resolution()
}

/// Subscribe to the monotonic clock.
/// If `absolute` is true, subscribes to an absolute instant.
/// If false, subscribes to a duration from now.
/// Returns a raw pollable handle (i32).
#[no_mangle]
pub extern "C" fn host_api_monotonic_clock_subscribe(when: u64, absolute: bool) -> i32 {
    let pollable: Pollable = if absolute {
        monotonic_clock::subscribe_instant(when)
    } else {
        monotonic_clock::subscribe_duration(when)
    };
    let handle = pollable.handle() as i32;
    core::mem::forget(pollable); // Caller takes ownership of the handle.
    handle
}
