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

    // Include SM's config header in all compilations
    if confdefs.exists() {
        build.flag(&format!("-include{}", confdefs.display()));
    }

    build.include(&sm_include);

    for src in &shim_sources {
        build.file(src);
    }

    build.compile("sm_shim");

    // Link the SpiderMonkey static library.
    println!(
        "cargo:rustc-link-search=native={}",
        sm_dir.display()
    );
    println!("cargo:rustc-link-lib=static=spidermonkey");

    // Expose the SM include path to downstream crates via DEP_SPIDERMONKEY_INCLUDE.
    println!(
        "cargo:include={}",
        sm_include.display()
    );

    // Rerun if shim sources change.
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-env-changed=SPIDERMONKEY_DIR");
    println!("cargo:rerun-if-env-changed=SPIDERMONKEY_BINARIES");
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

    eprintln!(
        "WARNING: Could not auto-detect SpiderMonkey location. \
         Set SPIDERMONKEY_DIR to the directory containing libspidermonkey.a."
    );

    // Return a placeholder path that will fail the assertion in main().
    workspace_root.join("deps").join("spidermonkey")
}
