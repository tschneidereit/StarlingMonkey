use std::env;
use std::path::PathBuf;

fn main() {
    let sm_dir = find_spidermonkey();
    let sm_include = sm_dir.join("include");
    let sm_lib = sm_dir.join("libspidermonkey.a");

    assert!(
        sm_lib.exists(),
        "SpiderMonkey library not found at {}. \
         Set SPIDERMONKEY_DIR to the directory containing libspidermonkey.a \
         (or SPIDERMONKEY_BINARIES for compatibility with the old CMake build).",
        sm_lib.display()
    );

    // Find wasi-sdk for C++ compilation when targeting wasm32.
    let wasi_sdk = find_wasi_sdk();

    // Compile the C++ shim files.
    let shim_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("shim");

    let shim_sources: Vec<PathBuf> = std::fs::read_dir(&shim_dir)
        .expect("shim/ directory must exist")
        .filter_map(|e| {
            let path = e.ok()?.path();
            if path.extension().map_or(false, |ext| ext == "cpp") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    assert!(
        !shim_sources.is_empty(),
        "No .cpp files found in {}",
        shim_dir.display()
    );

    let confdefs = sm_include.join("js-confdefs.h");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("gnu++20")
        .warnings(false)
        // SM requires these flags for correct object layout on wasm32
        .flag("-fno-rtti")
        .flag("-fno-exceptions")
        .flag("-fno-sized-deallocation")
        .flag("-fno-aligned-new")
        .flag("-m32")
        .flag("-fPIC")
        .flag("-fno-math-errno")
        .flag("-mthread-model")
        .flag("single");
    if cfg!(feature = "debugmozjs") {
        build.flag("-DJS_GC_ZEAL").flag("-DDEBUG").flag("-DJS_DEBUG");
    }

    // If we have wasi-sdk, configure the compiler to use it.
    if let Some(ref wasi_sdk_dir) = wasi_sdk {
        let clangxx = wasi_sdk_dir.join("bin").join("clang++");
        if clangxx.exists() {
            build.compiler(&clangxx);
        }
    }

    // Include SM's config header in all compilations
    if confdefs.exists() {
        build.flag(&format!("-include{}", confdefs.display()));
    }

    build.include(&sm_include);

    for src in &shim_sources {
        build.file(src);
    }

    build.compile("sm_shim");

    // // Link the SpiderMonkey static library.
    // println!(
    //     "cargo:rustc-link-search=native={}",
    //     sm_dir.display()
    // );
    // println!("cargo:rustc-link-lib=static=spidermonkey");

    // Expose the SM include path to downstream crates via DEP_SPIDERMONKEY_INCLUDE.
    println!(
        "cargo:include={}",
        sm_include.display()
    );

    // Expose wasi-sdk path to downstream crates.
    if let Some(ref wasi_sdk_dir) = wasi_sdk {
        println!("cargo:wasi_sdk={}", wasi_sdk_dir.display());
    }

    // Rerun if shim sources change.
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-env-changed=SPIDERMONKEY_DIR");
    println!("cargo:rerun-if-env-changed=SPIDERMONKEY_BINARIES");
    println!("cargo:rerun-if-env-changed=WASI_SDK_DIR");
}

/// Locate the SpiderMonkey build artifacts directory.
///
/// Checks in order:
/// 1. `SPIDERMONKEY_DIR` env var
/// 2. `SPIDERMONKEY_BINARIES` env var (CMake compat)
/// 3. Well-known paths relative to the workspace root
fn find_spidermonkey() -> PathBuf {
    if let Ok(dir) = env::var("SPIDERMONKEY_DIR") {
        return PathBuf::from(dir);
    }

    if let Ok(dir) = env::var("SPIDERMONKEY_BINARIES") {
        return PathBuf::from(dir);
    }

    // Try well-known locations from the existing CMake build.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();

    // Check cmake-build-p3 deps (where CPM stores downloaded SM).
    let candidates = [
        "cmake-build-p3",
        "cmake-build-p3-release",
        "cmake-build-p3-debug",
    ];

    for build_dir in &candidates {
        let deps = workspace_root.join(build_dir).join("deps");
        if deps.exists() {
            // Look for spidermonkey-* directories from CPM
            if let Ok(entries) = std::fs::read_dir(&deps) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("spidermonkey-") {
                        let candidate = entry.path();
                        if candidate.join("libspidermonkey.a").exists() {
                            return candidate;
                        }
                    }
                }
            }
        }
    }

    // Also check deps/cpm_cache/spidermonkey-*
    // When debugmozjs feature is enabled, prefer the debug SM build.
    let cpm_cache = workspace_root.join("deps").join("cpm_cache");
    let prefixes: &[&str] = if cfg!(feature = "debugmozjs") {
        &["spidermonkey-debug", "spidermonkey-release"]
    } else {
        &["spidermonkey-release", "spidermonkey-debug"]
    };
    for prefix in prefixes {
        let dir = cpm_cache.join(prefix);
        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let candidate = entry.path();
                    if candidate.join("libspidermonkey.a").exists() {
                        return candidate;
                    }
                }
            }
        }
    }

    eprintln!(
        "WARNING: Could not auto-detect SpiderMonkey location. \
         Set SPIDERMONKEY_DIR to the directory containing libspidermonkey.a."
    );

    // Return a placeholder path that will fail the assertion in main().
    workspace_root.join("deps").join("spidermonkey")
}

/// Locate the wasi-sdk installation.
///
/// Checks in order:
/// 1. `WASI_SDK_DIR` env var
/// 2. Well-known paths in deps/cpm_cache/wasi-sdk/
fn find_wasi_sdk() -> Option<PathBuf> {
    if let Ok(dir) = env::var("WASI_SDK_DIR") {
        let p = PathBuf::from(dir);
        if p.join("bin").join("clang++").exists() {
            return Some(p);
        }
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();

    let wasi_sdk_cache = workspace_root.join("deps").join("cpm_cache").join("wasi-sdk");
    if wasi_sdk_cache.exists() {
        if let Ok(entries) = std::fs::read_dir(&wasi_sdk_cache) {
            for entry in entries.flatten() {
                let candidate = entry.path();
                if candidate.join("bin").join("clang++").exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}
