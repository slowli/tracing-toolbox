# Tracing Infrastructure for Tardigrade Workflows

[![Build Status](https://github.com/slowli/tardigrade/workflows/CI/badge.svg?branch=main)](https://github.com/slowli/tardigrade/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue)](https://github.com/slowli/tardigrade#license)
![rust 1.60+ required](https://img.shields.io/badge/rust-1.60+-blue.svg?label=Required%20Rust)

**Documentation:**
[![crate docs (main)](https://img.shields.io/badge/main-yellow.svg?label=docs)](https://slowli.github.io/tardigrade/tardigrade_tracing/)

This crate provides various [tracing] infrastructure helpers for [`tardigrade`]
workflows.

- `TracingEventSender` is a tracing [`Subscriber`] that converts tracing events
  into (de)serializable presentation that can be sent elsewhere using a customizable hook.
  The [`tardigrade`] client library uses this subscriber to send tracing events to the host.
- `TracingEventReceiver` consumes events produced by a `TracingEventSender` and relays them
  to the tracing infrastructure. The consumer is used by [the Tardigrade runtime].
- `CaptureLayer` can be used to capture spans during testing.

## Usage

Add this to your `Crate.toml`:

```toml
[dependencies]
tardigrade-tracing = "0.1.0"
```

Note that the 3 pieces of functionality described above are gated behind opt-in features;
consult the crate docs for details.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `tardigrade` by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[`tardigrade`]: https://crates.io/crates/tardigrade
[tracing]: https://docs.rs/tracing/0.1/tracing
[`Subscriber`]: https://docs.rs/tracing-core/0.1/tracing_core/trait.Subscriber.html
[the Tardigrade runtime]: https://crates.io/crates/tardigrade-rt
