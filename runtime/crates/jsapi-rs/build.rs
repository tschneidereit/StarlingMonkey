fn main() {
    let wasi_sdk = std::env::var("WASI_SDK_PATH").unwrap_or_else(|_| "/opt/wasi-sdk".to_string());
    if !std::path::Path::new(&wasi_sdk).exists() {
        println!("cargo:error=WASI_SDK_PATH environment variable is not set or points to a non-existent directory: {wasi_sdk}");
        return;
    }
    let mut cxx_flags = vec![
        "-DRUST_BINDGEN",
        "-DSTATIC_JS_API",
        "-std=gnu++20",
        "-Wall",
        "-Qunused-arguments",
        "-fno-sized-deallocation",
        "-fno-aligned-new",
        "-mthread-model", "single",
        "-fPIC",
        "-fno-rtti",
        "-fno-exceptions",
        "-fno-math-errno",
        "-pipe",
        "-fno-omit-frame-pointer",
        "-funwind-tables",
        "-m32",
        "-include",
        "js-confdefs.h",
    ];

    #[cfg(feature = "debugmozjs")]
    cxx_flags.extend(& ["-DJS_GC_ZEAL", "-DDEBUG", "-DJS_DEBUG"]);

    let mut build = cc::Build::new();
    build
        .target("wasm32-wasip1")
        .cpp(true)
        // TODO(ts): make this work wherever the headers are located
        .include("../libspidermonkey/include")
        .file("cpp/jsglue.cpp");

    build.flags(&cxx_flags);
    // for flag in cxx_flags.iter() {
    //     build.flag_if_supported(flag);
    // }

    unsafe {
        std::env::set_var("CXX_wasm32-wasip1", format!("{wasi_sdk}/bin/clang++"));
        std::env::set_var("WASI_SYSROOT", format!("{wasi_sdk}/share/wasi-sysroot"));
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();

    build.out_dir(&out_dir).compile("jsglue");

    println!("cargo:rustc-link-arg=--error-unresolved-symbols");
    println!("cargo:rustc-link-search=native=libspidermonkey");
    println!("cargo:rustc-link-lib=static=spidermonkey");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static=jsglue");
}
