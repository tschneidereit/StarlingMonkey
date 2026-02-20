use wasi::io::poll as wasi_poll;

/// Poll a list of pollable handles, returning the index of the first ready one.
/// `handles` is a pointer to an array of raw pollable handle i32 values.
/// `count` is the number of handles.
/// Returns the index of the first ready handle.
#[no_mangle]
pub unsafe extern "C" fn host_api_poll(handles: *const i32, count: usize) -> usize {
    let handle_slice = core::slice::from_raw_parts(handles, count);
    // Convert raw i32 handles to wasi Pollable borrows using unsafe transmute.
    // The wasi crate's Pollable is a wrapper around a resource handle (i32).
    let pollables: Vec<wasi_poll::Pollable> = handle_slice
        .iter()
        .map(|&h| wasi_poll::Pollable::from_handle(h as u32))
        .collect();

    let borrow_refs: Vec<&wasi_poll::Pollable> = pollables.iter().collect();
    let ready = wasi_poll::poll(&borrow_refs);

    // Don't drop the pollables — we don't own them, C++ does.
    for p in pollables {
        core::mem::forget(p);
    }

    // Return the smallest ready index to implement oldest-first scheduling.
    // The WASI poll spec doesn't guarantee ordering of the ready list,
    // but the event loop expects oldest-first (lowest index = oldest task).
    *ready.iter().min().unwrap() as usize
}

/// Block on a single pollable handle until it's ready.
#[no_mangle]
pub extern "C" fn host_api_pollable_block(handle: i32) {
    let pollable = unsafe { wasi_poll::Pollable::from_handle(handle as u32) };
    pollable.block();
    core::mem::forget(pollable); // Don't drop — C++ owns the handle.
}

/// Drop a pollable handle, releasing the underlying resource.
#[no_mangle]
pub extern "C" fn host_api_pollable_drop(handle: i32) {
    let pollable = unsafe { wasi_poll::Pollable::from_handle(handle as u32) };
    drop(pollable);
}
