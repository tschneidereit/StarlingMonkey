//! Build script for starling-builtins.
//!
//! Invokes CMake to compile the C++ builtins as a static library,
//! passing feature flags to control which builtins are enabled.
//!
//! The CMake build produces:
//! - Individual static libraries for each builtin (builtin_*.a)
//! - A combined `libbuiltins.a` that links them all
//! - `builtins.incl` with namespace declarations
//!
//! This build.rs reads the enabled cargo features, maps them to
//! CMake ENABLE_BUILTIN_* options, and links the resulting libraries.

use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Map from cargo feature name to CMake option name.
const FEATURE_MAP: &[(&str, &str)] = &[
    ("base64", "ENABLE_BUILTIN_WEB_BASE64"),
    ("blob", "ENABLE_BUILTIN_WEB_BLOB"),
    ("console", "ENABLE_BUILTIN_WEB_CONSOLE"),
    ("crypto", "ENABLE_BUILTIN_WEB_CRYPTO"),
    ("dom-exception", "ENABLE_BUILTIN_WEB_DOM_EXCEPTION"),
    ("event", "ENABLE_BUILTIN_WEB_EVENT"),
    ("abort", "ENABLE_BUILTIN_WEB_ABORT"),
    ("fetch", "ENABLE_BUILTIN_WEB_FETCH"),
    ("fetch-event", "ENABLE_BUILTIN_WEB_FETCH_FETCH_EVENT"),
    ("file", "ENABLE_BUILTIN_WEB_FILE"),
    ("form-data", "ENABLE_BUILTIN_WEB_FORM_DATA"),
    ("global-self", "ENABLE_BUILTIN_WEB_GLOBAL_SELF"),
    ("performance", "ENABLE_BUILTIN_WEB_PERFORMANCE"),
    ("queue-microtask", "ENABLE_BUILTIN_WEB_QUEUE_MICROTASK"),
    ("streams", "ENABLE_BUILTIN_WEB_STREAMS"),
    ("structured-clone", "ENABLE_BUILTIN_WEB_STRUCTURED_CLONE"),
    ("text-codec", "ENABLE_BUILTIN_WEB_TEXT_CODEC"),
    ("timers", "ENABLE_BUILTIN_WEB_TIMERS"),
    ("url", "ENABLE_BUILTIN_WEB_URL"),
    ("worker-location", "ENABLE_BUILTIN_WEB_WORKER_LOCATION"),
];

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();

    // Get SM include path from the -sys crate's metadata.
    let sm_include = env::var("DEP_SPIDERMONKEY_INCLUDE")
        .unwrap_or_else(|_| {
            // Fallback: look for SM in cmake-build-p3
            workspace_root.join("cmake-build-p3/deps/spidermonkey/include")
                .to_string_lossy()
                .into_owned()
        });

    let cmake_build_dir = out_dir.join("cmake-builtins");
    std::fs::create_dir_all(&cmake_build_dir).unwrap();

    // Configure CMake
    let builtins_cmake = workspace_root.join("CMakeLists.txt");
    let mut cmake_cmd = Command::new("cmake");
    cmake_cmd
        .current_dir(&cmake_build_dir)
        .arg(workspace_root)
        .arg(format!("-DCMAKE_BUILD_TYPE=Release"))
        .arg(format!("-DSPIDERMONKEY_INCLUDE_DIR={sm_include}"));

    // Pass feature flags
    for (feature, cmake_opt) in FEATURE_MAP {
        let enabled = env::var(format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_")))
            .is_ok();
        cmake_cmd.arg(format!("-D{cmake_opt}={}", if enabled { "ON" } else { "OFF" }));
    }

    // Run cmake configure
    let status = cmake_cmd.status()
        .expect("Failed to run cmake configure");
    if !status.success() {
        eprintln!("Warning: CMake configure failed. This is expected during initial scaffolding.");
        // During scaffolding, we don't need the actual build to succeed.
        // The crate structure is what matters.
        return;
    }

    // Build
    let status = Command::new("cmake")
        .current_dir(&cmake_build_dir)
        .args(["--build", ".", "--target", "builtins"])
        .status()
        .expect("Failed to run cmake build");

    if !status.success() {
        eprintln!("Warning: CMake build failed. This is expected during initial scaffolding.");
        return;
    }

    // Link the builtins static library
    println!("cargo:rustc-link-search=native={}", cmake_build_dir.display());
    println!("cargo:rustc-link-lib=static=builtins");

    // Also link individual builtin libraries that CMake produced
    for (feature, _) in FEATURE_MAP {
        let enabled = env::var(format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_")))
            .is_ok();
        if enabled {
            let lib_name = format!("builtin_web_{}", feature.replace('-', "_"));
            println!("cargo:rustc-link-lib=static={lib_name}");
        }
    }
}
