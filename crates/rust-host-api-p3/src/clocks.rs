use wasip3::clocks::monotonic_clock;

/// Get the current monotonic clock time in nanoseconds.
#[no_mangle]
pub extern "C" fn host_api_monotonic_clock_now() -> u64 {
    monotonic_clock::now()
}

/// Get the monotonic clock resolution in nanoseconds.
#[no_mangle]
pub extern "C" fn host_api_monotonic_clock_resolution() -> u64 {
    monotonic_clock::get_resolution()
}

/// Subscribe to the monotonic clock.
/// In WASIp3, there are no Pollable handles. Instead, we record the deadline
/// in a "waiter" slot and return its handle. The event loop (in Rust) will
/// await `wait_until(deadline)` when this waiter is selected.
///
/// If `absolute` is true, `when` is an absolute instant.
/// If false, `when` is a duration from now.
/// Returns a waiter handle (i32).
#[no_mangle]
pub extern "C" fn host_api_monotonic_clock_subscribe(when: u64, absolute: bool) -> i32 {
    let deadline = if absolute {
        when
    } else {
        monotonic_clock::now() + when
    };
    crate::poll::register_timer_waiter(deadline)
}
