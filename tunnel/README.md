# Tunnelling Tracing Information Across API Boundary

[![Build Status](https://github.com/slowli/tardigrade/workflows/CI/badge.svg?branch=main)](https://github.com/slowli/tardigrade/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue)](https://github.com/slowli/tardigrade#license)
![rust 1.60+ required](https://img.shields.io/badge/rust-1.60+-blue.svg?label=Required%20Rust)

**Documentation:**
[![crate docs (main)](https://img.shields.io/badge/main-yellow.svg?label=docs)](https://slowli.github.io/tardigrade/tardigrade_tracing/)

This crate provides [tracing] infrastructure helpers allowing to transfer tracing events
across API boundary:

- `TracingEventSender` is a tracing [`Subscriber`] that converts tracing events
  into (de)serializable presentation that can be sent elsewhere using a customizable hook.
- `TracingEventReceiver` consumes events produced by a `TracingEventSender` and relays them
  to the tracing infrastructure. It is assumed that the source of events may outlive
  both the lifetime of a particular `TracingEventReceiver` instance, and the lifetime
  of the program encapsulating the receiver. To deal with this, the receiver provides
  the means to persist / restore its state.

Both components are used by the [Tardigrade][`tardigrade`] workflows, in case of which
the API boundary is the WASM client–host boundary.

- The [`tardigrade`] client library uses `TracingEventSender` to send tracing events
  from a workflow (i.e., a WASM module instance) to the host using a WASM import function.
- [The Tardigrade runtime] uses `TracingEventReceiver` to pass traces from the workflow
  to the host tracing infrastructure.

## Usage

Add this to your `Crate.toml`:

```toml
[dependencies]
tracing-tunnel = "0.1.0"
```

Note that the 3 pieces of functionality described above are gated behind opt-in features;
consult the crate docs for details.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `tracing-tunnel` by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[`tardigrade`]: https://crates.io/crates/tardigrade
[tracing]: https://docs.rs/tracing/0.1/tracing
[`Subscriber`]: https://docs.rs/tracing-core/0.1/tracing_core/trait.Subscriber.html
[The Tardigrade runtime]: https://crates.io/crates/tardigrade-rt
