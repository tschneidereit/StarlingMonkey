/// FFI wrappers for `wasi::cli::environment`.

/// Get the number of CLI arguments.
#[no_mangle]
pub extern "C" fn host_api_cli_argc() -> u32 {
    wasi::cli::environment::get_arguments().len() as u32
}

/// Get CLI argument at `index`. Returns a C string in `*out_ptr` / `*out_len`.
/// Caller must free with `host_api_string_free`.
#[no_mangle]
pub extern "C" fn host_api_cli_argv(index: u32, out_ptr: *mut *mut u8, out_len: *mut usize) {
    let args = wasi::cli::environment::get_arguments();
    let arg = &args[index as usize];
    let mut boxed = arg.clone().into_bytes().into_boxed_slice();
    unsafe {
        *out_ptr = boxed.as_mut_ptr();
        *out_len = boxed.len();
    }
    core::mem::forget(boxed);
}

/// Free a string returned by cli functions.
#[no_mangle]
pub unsafe extern "C" fn host_api_string_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let _ = Vec::from_raw_parts(ptr, len, len);
    }
}
