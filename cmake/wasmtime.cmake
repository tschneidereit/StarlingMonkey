if (DEFINED ENV{WASMTIME_DIR})
    set(WASMTIME_DIR $ENV{WASMTIME_DIR})
    return()
endif ()

set(WASMTIME_VERSION v16.0.0)
set(WASMTIME_URL https://github.com/bytecodealliance/wasmtime/releases/download/${WASMTIME_VERSION}/wasmtime-${WASMTIME_VERSION}-${HOST_ARCH}-${HOST_OS}.tar.xz)
CPMAddPackage(NAME wasmtime URL ${WASMTIME_URL} DOWNLOAD_ONLY TRUE)
set(WASMTIME_DIR ${CPM_PACKAGE_wasmtime_SOURCE_DIR})
