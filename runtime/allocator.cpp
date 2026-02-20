#include "allocator.h"
#include <cstdlib>

JSContext *CONTEXT = nullptr;

extern "C" {

// Use standard realloc rather than JS_realloc. Buffers allocated here are
// consumed by wasip2-generated code which frees them with the standard
// allocator (via Vec::drop / dealloc), or passed to C++ code that frees
// with delete[]/free. Using JS_realloc would cause allocator mismatches
// in SpiderMonkey debug builds.
__attribute__((weak, export_name("cabi_realloc"))) void *cabi_realloc(void *ptr, size_t orig_size,
                                                                size_t _align, size_t new_size) {
  if (new_size == 0) {
    free(ptr);
    // Return a non-null aligned dangling pointer for zero-size allocations.
    // The wasip2 crate's generated code passes returned pointers to
    // Vec::from_raw_parts, which panics with null in debug builds.
    return reinterpret_cast<void *>(_align);
  }
  if (new_size == orig_size) {
    return ptr;
  }
  return realloc(ptr, new_size);
}

void cabi_free(void *ptr) { free(ptr); }
}
