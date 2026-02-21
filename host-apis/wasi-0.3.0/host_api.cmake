# WASIp3 host API implementation
# All C++ host_api code and the Rust rust-host-api-p3 crate.

add_library(host_api STATIC
        ${HOST_API}/host_api.cpp
        ${HOST_API}/host_call.cpp
        ${HOST_API}/sockets.cpp
        ${HOST_API}/event_loop.cpp
        ${HOST_API}/request_handler.cpp
)

target_link_libraries(host_api PRIVATE spidermonkey rust-crates)
target_include_directories(host_api PRIVATE include runtime deps/include
        ${CMAKE_CURRENT_SOURCE_DIR}/builtins/web/fetch
        ${CMAKE_CURRENT_SOURCE_DIR}/builtins/web)
target_include_directories(host_api PUBLIC ${HOST_API}/include)

# Tell build-crates.cmake which Rust crate to use
set(RUST_HOST_API_CRATE "rust-host-api-p3")
set(RUST_HOST_API_CRATE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/crates/rust-host-api-p3")
