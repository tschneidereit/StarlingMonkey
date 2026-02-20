#ifndef JS_COMPUTE_RUNTIME_ALLOCATOR_H
#define JS_COMPUTE_RUNTIME_ALLOCATOR_H

#include <cstdint>

struct JSContext;

/// We keep a handle to the JSContext for potential use.
extern JSContext *CONTEXT;

extern "C" {

/// A strong symbol to override the cabi_realloc defined by wit-bindgen. Uses
/// standard realloc so that buffers are compatible with both Rust's dealloc
/// and C++ delete[]/free.
void *cabi_realloc(void *ptr, size_t orig_size, size_t align, size_t new_size);

/// A more ergonomic version of cabi_realloc for fresh allocations.
inline void *cabi_malloc(size_t bytes, size_t align) { return cabi_realloc(nullptr, 0, align, bytes); }

/// Free memory allocated by cabi_realloc.
void cabi_free(void *ptr);
}

#endif
