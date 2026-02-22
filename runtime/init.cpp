/**
 * Engine initialization glue — minimal C++ shim.
 *
 * Most entry points (CLI run, init from environment, wizer init) are now
 * defined directly in Rust (entry.rs). This file only provides:
 *   1. clock_gettime override (weak C symbol) for monotonic clock correctness
 *   2. WIZER_INIT macro (requires C++ for __wasm_call_ctors linkage)
 *   3. Weak main() stub for WASI reactor model
 */

#include <cerrno>
#include <ctime>
#include <cstdlib>
#include <wasi/api.h>

// ── Rust FFI ───────────────────────────────────────────────────────

extern "C" {
void starling_wizer_init();
}

// ── Clock monotonicity fix ─────────────────────────────────────────
//
// Override wasi-libc's weakly linked clock_gettime to ensure monotonic
// clocks really are monotonic across wizer snapshot resumptions.
// The actual offset is managed by Rust (entry.rs), but we need to read
// it here. For simplicity, we maintain a parallel C-side copy which the
// Rust wizer init sets before return.

static uint64_t mono_clock_offset = 0;

extern "C" {
typedef uint32_t __wasi_clockid_t;
typedef uint64_t __wasi_timestamp_t;
__wasi_errno_t __wasi_clock_time_get(__wasi_clockid_t, __wasi_timestamp_t,
                                      __wasi_timestamp_t *);
}

#define NSECS_PER_SEC 1000000000

int clock_gettime(clockid_t clock, timespec *ts) {
  __wasi_clockid_t clock_id = 0;
  if (clock == CLOCK_REALTIME) {
    clock_id = __WASI_CLOCKID_REALTIME;
  } else if (clock == CLOCK_MONOTONIC) {
    clock_id = __WASI_CLOCKID_MONOTONIC;
  } else {
    return EINVAL;
  }
  __wasi_timestamp_t t = 0;
  auto err = __wasi_clock_time_get(clock_id, 1, &t);
  if (err != 0) {
    return EINVAL;
  }
  if (clock == CLOCK_MONOTONIC) {
    t += mono_clock_offset;
  }
  ts->tv_sec = t / NSECS_PER_SEC;
  ts->tv_nsec = t % NSECS_PER_SEC;
  return 0;
}

/// Called from Rust wizer init to update the C-side mono_clock_offset.
extern "C" void starling_set_mono_clock_offset(uint64_t offset) {
  mono_clock_offset = offset;
}

// ── Entry points ───────────────────────────────────────────────────

__attribute__((weak))
int main(int argc, const char *argv[]) {
  // Should not be called — WASI reactor model.
  return 1;
}

// Wizer pre-initialization entry point.
static void wizen() {
  starling_wizer_init();
}

#include "wizer.h"
WIZER_INIT(wizen);
