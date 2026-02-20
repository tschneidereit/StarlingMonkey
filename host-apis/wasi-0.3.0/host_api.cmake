# WASIp3 host API implementation
# Uses the same C++ code as wasi-0.2.3 (same Rust FFI interface)
# but backed by rust-host-api-p3 (wasip3 crate).
# The event loop and request handler are p3-specific (Rust-driven async loop).

set(WASI_P2_DIR ${CMAKE_CURRENT_SOURCE_DIR}/host-apis/wasi-0.2.3)

add_library(host_api STATIC
        ${WASI_P2_DIR}/host_api.cpp
        ${WASI_P2_DIR}/host_call.cpp
        ${WASI_P2_DIR}/sockets.cpp
        ${HOST_API}/event_loop.cpp
        ${HOST_API}/request_handler.cpp
)

target_link_libraries(host_api PRIVATE spidermonkey rust-crates)
target_include_directories(host_api PRIVATE include runtime deps/include
        ${CMAKE_CURRENT_SOURCE_DIR}/builtins/web/fetch
        ${CMAKE_CURRENT_SOURCE_DIR}/builtins/web)
target_include_directories(host_api PUBLIC ${WASI_P2_DIR}/include)

# Tell build-crates.cmake which Rust crate to use
set(RUST_HOST_API_CRATE "rust-host-api-p3")
set(RUST_HOST_API_CRATE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/crates/rust-host-api-p3")
