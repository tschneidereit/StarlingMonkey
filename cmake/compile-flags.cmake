set(WASI 1)
set(CMAKE_CXX_STANDARD 20)
add_compile_definitions("$<$<CONFIG:DEBUG>:DEBUG=1>")

# Force compiler checks to pass — the wasip2 linker requires _initialize
# which the test program doesn't provide, causing false negatives.
set(CMAKE_C_COMPILER_WORKS TRUE)
set(CMAKE_CXX_COMPILER_WORKS TRUE)

# Base linker flags safe for the cmake compiler test.
list(APPEND CMAKE_EXE_LINKER_FLAGS
        -Wl,-z,stack-size=1048576 -Wl,--stack-first
        -Wl,--skip-wit-component
        -Wl,--component-type,${CMAKE_CURRENT_SOURCE_DIR}/wizer.wit
        -nostartfiles
        -mexec-model=reactor
        -lwasi-emulated-getpid -lwasi-emulated-signal
        -lwasi-emulated-mman -lwasi-emulated-process-clocks
)

# Export the correct WASI interface symbols based on the host API version.
if (HOST_API MATCHES "wasi-0\\.3")
    list(APPEND CMAKE_EXE_LINKER_FLAGS
        "\"-Wl,--export=[async-lift]wasi:http/handler@0.3.0-rc-2026-01-06#handle\""
        "\"-Wl,--export=[callback][async-lift]wasi:http/handler@0.3.0-rc-2026-01-06#handle\""
        "\"-Wl,--export=[async-lift]wasi:cli/run@0.3.0-rc-2026-01-06#run\""
        "\"-Wl,--export=[callback][async-lift]wasi:cli/run@0.3.0-rc-2026-01-06#run\""
        "-Wl,--export=cabi_realloc"
    )
else()
    list(APPEND CMAKE_EXE_LINKER_FLAGS
        "-Wl,--export=wasi:http/incoming-handler@0.2.9#handle"
        "-Wl,--export=wasi:cli/run@0.2.9#run"
    )
endif()
list(JOIN CMAKE_EXE_LINKER_FLAGS " " CMAKE_EXE_LINKER_FLAGS)

list(APPEND CMAKE_CXX_FLAGS
        -std=gnu++20 -Wall -Werror -Qunused-arguments
        -Wimplicit-fallthrough -Wno-unknown-warning-option -Wno-invalid-offsetof
        -fno-sized-deallocation -fno-aligned-new -mthread-model single
        -fPIC -fno-rtti -fno-exceptions -fno-math-errno -pipe
        -fno-omit-frame-pointer -funwind-tables -m32
)
list(JOIN CMAKE_CXX_FLAGS " " CMAKE_CXX_FLAGS)

list(APPEND CMAKE_C_FLAGS
        -Wall -Werror -Wno-unknown-attributes -Wno-pointer-to-int-cast
        -Wno-int-to-pointer-cast -m32
)
list(JOIN CMAKE_C_FLAGS " " CMAKE_C_FLAGS)
