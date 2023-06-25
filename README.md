# Toolbox for Tracing in Rust

[![Build Status](https://github.com/slowli/tracing-toolbox/workflows/CI/badge.svg?branch=main)](https://github.com/slowli/tracing-toolbox/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue)](https://github.com/slowli/tracing-toolbox#license)

This repository provides various small(ish) tools for Rust [tracing] infrastructure.
Currently, the following tools are included:

- [`tracing-tunnel`](tunnel): Provides a tunnel to pass tracing information across
  an API boundary (such as the WASM client–host boundary).
- [`tracing-capture`](capture): Allows capturing tracing spans and events,
  e.g. to use in test assertions.
- [`tracing-metrics-recorder`](metrics-recorder): Converts metrics collected using
  [the eponymous facade][metrics] into tracing events

## License

All code is licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `tracing-toolbox` by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[tracing]: https://docs.rs/tracing/0.1/tracing
[metrics]: https://docs.rs/metrics/
