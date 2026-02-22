ncpus := num_cpus()
justdir := justfile_directory()
mode := 'debug'
builddir := justdir / 'cmake-build-p3-' + mode
wpt_root := justdir / 'deps' / 'wpt-source'
reconfigure := 'false'
target_dir := justdir / 'target' / 'wasm32-wasip2' / mode

alias b := build
alias t := test
alias w := wpt-test
alias c := componentize
alias fmt := format

# List all recipes
default:
    @echo 'Default mode {{ mode }}'
    @echo 'Default build directory {{ builddir }}'
    @echo 'Default cargo target dir {{ target_dir }}'
    @just --list

# ── Cargo-centric build (new) ──────────────────────────────────────

# Build the starling wasm binary via Cargo
build *flags:
    cargo build {{ if mode == "release" { "--release" } else { "" } }} {{ flags }}

# Build a specific crate
build-crate crate *flags:
    cargo build -p {{ crate }} {{ if mode == "release" { "--release" } else { "" } }} {{ flags }}

# Clean Cargo build artifacts
clean:
    cargo clean

# ── CMake legacy build (for builtins development) ──────────────────

# Build specified target via CMake (for builtins/host-api C++ development)
cmake-build target="all" *flags:
    #!/usr/bin/env bash
    set -euo pipefail
    echo 'Setting build directory to {{ builddir }}, build type {{ mode }}'

    # Only run configure step if build directory doesn't exist yet
    if ! {{ path_exists(builddir) }} || {{ reconfigure }} = 'true'; then
        cmake -S . -B {{ builddir }} {{ flags }} -DCMAKE_BUILD_TYPE={{ capitalize(mode) }}
    else
        echo 'build directory already exists, skipping cmake configure'
    fi

    # Build target
    cmake --build {{ builddir }} --parallel {{ ncpus }} {{ if target == "" { "" } else { "--target " + target } }}

# Run CMake clean target
cmake-clean:
    cmake --build {{ builddir }} --target clean

[private]
[confirm('proceed?')]
do_clean:
    rm -rf {{ builddir }}

# Remove CMake build directory
clean-all: && do_clean
    @echo "This will remove {{builddir}}"

# Run clang-tidy
lint: (cmake-build "clang-tidy")

# Run clang-tidy and apply offered fixes
lint-fix: (cmake-build "clang-tidy-fix")

# Componentize js script (uses cmake-generated componentize.sh for now)
componentize script="" outfile="starling.wasm": (cmake-build "starling-raw.wasm")
    {{ builddir }}/componentize.sh {{ script }} -o {{ outfile }}

# Componentize and serve script with wasmtime
serve script: (componentize script)
    wasmtime serve -W component-model-async=y -S p3=y -S common starling.wasm

# Format code using clang-format. Use --fix to fix files inplace
format *ARGS:
    {{ justdir }}/scripts/clang-format.sh {{ ARGS }}

# Run integration test
test regex="": (cmake-build "integration-test-server") (cmake-build "wpt-runtime")
    ctest --test-dir {{ builddir }} -j {{ ncpus }} --output-on-failure {{ if regex == "" { regex } else { "-R " + regex } }}

# Run web platform test suite
[group('wpt')]
[arg("external-wpt", long)]
wpt-test filter="" external-wpt="false": (cmake-build "wpt-runtime")
    #!/usr/bin/env bash
    set -euo pipefail
    cd {{ builddir }}
    WASMTIME_BACKTRACE_DETAILS=1 node "{{ justdir }}/tests/wpt-harness/run-wpt.mjs" "--wpt-root={{ wpt_root }}" --external-wpt-server={{external-wpt}} -vv "{{ filter }}"

# Update web platform test expectations
[group('wpt')]
[arg("external-wpt", long)]
wpt-update filter="" external-wpt="false": (cmake-build "wpt-runtime")
    #!/usr/bin/env bash
    set -euo pipefail
    cd {{ builddir }}
    WASMTIME_BACKTRACE_DETAILS=1 node "{{ justdir }}/tests/wpt-harness/run-wpt.mjs" "--wpt-root={{ wpt_root }}" --external-wpt-server={{external-wpt}} -vv --update-expectations "{{ filter }}"

# Run wpt server
[group('wpt')]
wpt-server: (cmake-build "wpt-runtime")
    #!/usr/bin/env bash
    set -euo pipefail
    cd {{ builddir }}

    echo "Using wpt-suite at {{ wpt_root }}"
    WASMTIME_BACKTRACE_DETAILS= node "{{ justdir }}/tests/wpt-harness/run-wpt.mjs" "--wpt-root={{ wpt_root }}" -vv --interactive

# Prepare WPT hosts
[group('wpt')]
wpt-setup:
    cat deps/wpt-hosts | sudo tee -a /etc/hosts
