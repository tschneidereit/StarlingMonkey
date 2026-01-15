# Crossbeam Channel

[![Build Status](https://github.com/crossbeam-rs/crossbeam/workflows/CI/badge.svg)](
https://github.com/crossbeam-rs/crossbeam/actions)
[![License](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-blue.svg)](
https://github.com/crossbeam-rs/crossbeam/tree/master/crossbeam-channel#license)
[![Cargo](https://img.shields.io/crates/v/crossbeam-channel.svg)](
https://crates.io/crates/crossbeam-channel)
[![Documentation](https://docs.rs/crossbeam-channel/badge.svg)](
https://docs.rs/crossbeam-channel)
[![Rust 1.60+](https://img.shields.io/badge/rust-1.60+-lightgray.svg)](
https://www.rust-lang.org)
[![chat](https://img.shields.io/discord/569610676205781012.svg?logo=discord)](https://discord.com/invite/JXYwgWZ)

This crate provides multi-producer multi-consumer channels for message passing.
It is an alternative to [`std::sync::mpsc`] with more features and better performance.

Some highlights:

* [`Sender`]s and [`Receiver`]s can be cloned and shared among threads.
* Two main kinds of channels are [`bounded`] and [`unbounded`].
* Convenient extra channels like [`after`], [`never`], and [`tick`].
* The [`select!`] macro can block on multiple channel operations.
* [`Select`] can select over a dynamically built list of channel operations.
* Channels use locks very sparingly for maximum [performance](benchmarks).

[`std::sync::mpsc`]: https://doc.rust-lang.org/std/sync/mpsc/index.html
[`Sender`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/struct.Sender.html
[`Receiver`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/struct.Receiver.html
[`bounded`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/fn.bounded.html
[`unbounded`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/fn.unbounded.html
[`after`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/fn.after.html
[`never`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/fn.never.html
[`tick`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/fn.tick.html
[`select!`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/macro.select.html
[`Select`]: https://docs.rs/crossbeam-channel/*/crossbeam_channel/struct.Select.html

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
crossbeam-channel = "0.5"
```

## Single-Thread Mode

This fork includes a `single-thread` feature that enables callback-based message
delivery instead of blocking `recv()` calls. This is useful for single-threaded
environments where you want to avoid spawning threads.

### Enabling Single-Thread Mode

```toml
[dependencies]
crossbeam-channel = { version = "0.5", features = ["single-thread"] }
```

### Using Callbacks

```rust
use crossbeam_channel::unbounded;

let (s, r) = unbounded();

// Register a callback to receive messages
r.register_callback(|msg: i32| {
    println!("Received: {}", msg);
});

// Messages are now delivered synchronously via the callback
s.send(42).unwrap();
```

### Limitations in Single-Thread Mode

When the `single-thread` feature is enabled:

1. **`select!` macro is disabled**: Code using `select!` will not compile. You must
   register separate callbacks for each receiver instead.

2. **Timer channels are disabled**: The `after()`, `tick()`, and `never()` functions
   are not available. Calling `register_callback()` on timer-flavored receivers will panic.

3. **`Select` struct is disabled**: Dynamic selection over multiple channels is not
   supported. Use separate callbacks instead.

### Migration from `select!`

If you have code using `select!` like this:

```rust
// This won't compile in single-thread mode
select! {
    recv(r1) -> msg => handle_r1(msg),
    recv(r2) -> msg => handle_r2(msg),
}
```

Migrate it to use separate callbacks:

```rust
r1.register_callback(|msg| handle_r1(msg));
r2.register_callback(|msg| handle_r2(msg));
```

Note that unlike `select!`, which processes one message at a time from any ready
channel, callbacks are invoked independently and may run in any order.

## Compatibility

Crossbeam Channel supports stable Rust releases going back at least six months,
and every time the minimum supported Rust version is increased, a new minor
version is released. Currently, the minimum supported Rust version is 1.60.

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

#### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

#### Third party software

This product includes copies and modifications of software developed by third parties:

* [examples/matching.rs](examples/matching.rs) includes
  [matching.go](http://www.nada.kth.se/~snilsson/concurrency/src/matching.go) by Stefan Nilsson,
  licensed under Creative Commons Attribution 3.0 Unported License.

* [tests/mpsc.rs](tests/mpsc.rs) includes modifications of code from The Rust Programming Language,
  licensed under the MIT License and the Apache License, Version 2.0.

* [tests/golang.rs](tests/golang.rs) is based on code from The Go Programming Language, licensed
  under the 3-Clause BSD License.

See the source code files for more details.

Copies of third party licenses can be found in [LICENSE-THIRD-PARTY](LICENSE-THIRD-PARTY).
