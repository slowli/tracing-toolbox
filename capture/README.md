# Capturing Tracing Spans and Events

[![Build Status](https://github.com/slowli/tracing-toolbox/workflows/CI/badge.svg?branch=main)](https://github.com/slowli/tracing-toolbox/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue)](https://github.com/slowli/tracing-toolbox#license)
![rust 1.64+ required](https://img.shields.io/badge/rust-1.64+-blue.svg?label=Required%20Rust)

**Documentation:** [![Docs.rs](https://docs.rs/tracing-capture/badge.svg)](https://docs.rs/tracing-capture/)
[![crate docs (main)](https://img.shields.io/badge/main-yellow.svg?label=docs)](https://slowli.github.io/tracing-toolbox/tracing_capture/)

This crate provides a [tracing] [`Layer`] to capture tracing spans 
and events as they occur. The captured spans and events
can then be used for testing assertions (e.g., "Did a span
with a specific name / target / … occur? What were its fields? Was the span closed?
How many times the span was entered?" and so on).

The crate supports both straightforward assertions on the captured data,
and more fluent assertions based on [`predicates`].

## Usage

Add this to your `Crate.toml`:

```toml
[dependencies]
tracing-capture = "0.1.0"
```

### Capturing spans for test assertions

```rust
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_capture::{CaptureLayer, SharedStorage};

let subscriber = tracing_subscriber::fmt()
    .pretty()
    .with_max_level(Level::INFO)
    .finish();
// Add the capturing layer.
let storage = SharedStorage::default();
let subscriber = subscriber.with(CaptureLayer::new(&storage));

// Capture tracing information.
tracing::subscriber::with_default(subscriber, || {
    tracing::info_span!("test", num = 42_i64).in_scope(|| {
        tracing::warn!("I feel disturbance in the Force...");
    });
});

// Inspect the only captured span.
let storage = storage.lock();
assert_eq!(storage.all_spans().len(), 1);
let span = storage.all_spans().next().unwrap();
assert_eq!(span["num"], 42_i64);
assert_eq!(span.stats().entered, 1);
assert!(span.stats().is_closed);
```

### Predicate-based assertions

```rust
use predicates::str::contains;
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tracing_capture::{predicates::*, CaptureLayer, SharedStorage};

let storage = SharedStorage::default();
let subscriber = Registry::default().with(CaptureLayer::new(&storage));
tracing::subscriber::with_default(subscriber, || {
    tracing::info_span!("test_spans").in_scope(|| {
        tracing::warn!(result = 42_i64, "computed");
    });
});

let storage = storage.lock();
let predicate = level(Level::WARN)
    & message(contains("compute"))
    & field("result", 42_i64);
// Checks that there is a single event satisfying `predicate`.
storage.scan_events().single(&predicate);
// ...and that none of spans satisfy similar predicate.
storage.scan_spans().none(&level(Level::WARN));
```

## Alternatives / similar tools

- [`tracing-test`] is a lower-level alternative.
- [`tracing-fluent-assertions`] is more similar in intended goals, but differs significantly
  in API design; the assertions need to be declared before the capture.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `tracing-toolbox` by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[tracing]: https://docs.rs/tracing/0.1/tracing
[`Layer`]: https://docs.rs/tracing-subscriber/0.3/tracing_subscriber/trait.Layer.html
[`predicates`]: https://docs.rs/predicates/2/predicates
[`tracing-test`]: https://crates.io/crates/tracing-test
[`tracing-fluent-assertions`]: https://crates.io/crates/tracing-fluent-assertions
