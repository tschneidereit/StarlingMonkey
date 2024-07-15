corrosion_import_crate(MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/crates/rust-url/Cargo.toml NO_LINKER_OVERRIDE)

corrosion_import_crate(
        MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/runtime/crates/Cargo.toml
        NO_LINKER_OVERRIDE
        CRATE_TYPES "staticlib"
        IMPORTED_CRATES crates_list
)

#list(REMOVE_ITEM crates_list "generate-bindings")
foreach (crate IN LISTS crates_list)
    if (crate STREQUAL "generate-bindings")
        continue()
    endif ()
    add_dependencies("cargo-prebuild_${crate}" cargo-build_generate-bindings)
endforeach ()

message(STATUS "Imported crates: ${crates_list}")

add_library(rust-glue STATIC ${CMAKE_CURRENT_SOURCE_DIR}/runtime/crates/jsapi-rs/cpp/jsglue.cpp)
target_include_directories(rust-glue PRIVATE ${SM_INCLUDE_DIR})
add_dependencies(cargo-prebuild_generate-bindings rust-glue)

corrosion_set_env_vars(generate-bindings
        LIBCLANG_PATH=/opt/homebrew/opt/llvm/lib
        SYSROOT=${WASI_SDK_PREFIX}/share/wasi-sysroot
        CXXFLAGS="${CMAKE_CXX_FLAGS}"
        BIN_DIR=${CMAKE_CURRENT_BINARY_DIR}
        SM_HEADERS=${SM_INCLUDE_DIR}
        RUST_LOG=bindgen
)

set_property(TARGET rust-url PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${CMAKE_CURRENT_SOURCE_DIR}/crates/rust-url/)
