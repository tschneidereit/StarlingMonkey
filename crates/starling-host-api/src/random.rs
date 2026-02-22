use wasip3::random::random as wasi_random;

/// FFI result for getting random bytes.
#[repr(C)]
pub struct HostApiBytes {
    pub ptr: *mut u8,
    pub len: usize,
}

extern "C" {
    fn malloc(size: usize) -> *mut u8;
}

/// Get `len` random bytes. Caller takes ownership of the returned buffer
/// and must free it with standard free/delete[].
#[no_mangle]
pub extern "C" fn host_api_random_get_bytes(len: usize, out: *mut HostApiBytes) {
    let bytes = wasi_random::get_random_bytes(len as u64);
    // Allocate with libc malloc so C++ can free with delete[]/free.
    let ptr = unsafe { malloc(len) };
    if !ptr.is_null() && len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len) };
    }
    unsafe {
        *out = HostApiBytes { ptr, len };
    };
}

/// Get a random u32.
#[no_mangle]
pub extern "C" fn host_api_random_get_u32() -> u32 {
    wasi_random::get_random_u64() as u32
}

/// Free a HostApiBytes buffer previously returned by a host_api function.
#[no_mangle]
pub unsafe extern "C" fn host_api_bytes_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        let _ = Vec::from_raw_parts(ptr, len, len);
    }
}
