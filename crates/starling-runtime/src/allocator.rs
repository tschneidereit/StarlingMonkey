//! WASI Component ABI allocator.
//!
//! Replaces `runtime/allocator.cpp`. Provides `cabi_realloc` and `cabi_free`
//! for the component model's canonical ABI.

extern "C" {
    fn realloc(ptr: *mut u8, size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
}

/// Component ABI realloc function.
///
/// Used by WASIp2/p3 generated code to allocate/reallocate/free buffers that
/// cross the component boundary. Uses the standard C allocator to match what
/// Vec::drop / dealloc expect (avoiding allocator mismatches with SM's custom
/// allocator in debug builds).
#[no_mangle]
pub unsafe extern "C" fn cabi_realloc(
    ptr: *mut u8,
    orig_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    if new_size == 0 {
        if !ptr.is_null() {
            free(ptr);
        }
        // Return a non-null aligned dangling pointer for zero-size allocations.
        // The wasip2 crate's generated code passes returned pointers to
        // Vec::from_raw_parts, which panics with null in debug builds.
        return align as *mut u8;
    }
    if new_size == orig_size {
        return ptr;
    }
    realloc(ptr, new_size)
}

/// Component ABI free function.
#[no_mangle]
pub unsafe extern "C" fn cabi_free(ptr: *mut u8) {
    free(ptr);
}
