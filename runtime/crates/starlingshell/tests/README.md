# Starlingshell Tests

This directory contains JavaScript test files for the Starlingshell runtime.

## Running Tests

You can run any JavaScript file in this directory using:

```bash
cargo run --bin starlingshell -- tests/hello_world.js
```

Or from the workspace root:

```bash
cargo run --bin starlingshell -- runtime/crates/starlingshell/tests/hello_world.js
```

## Test Files

- **hello_world.js**: Basic test demonstrating console.log functionality with Servo's WebIDL bindings

## Usage Examples

### Run a script from file:
```bash
starlingshell tests/hello_world.js
```

### Execute inline JavaScript:
```bash
starlingshell -e "console.log('Hello, World!')"
```

### View help:
```bash
starlingshell --help
```

