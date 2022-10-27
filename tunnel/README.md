# Tunnelling Tracing Information Across API Boundary

[![Build Status](https://github.com/slowli/tracing-toolbox/workflows/CI/badge.svg?branch=main)](https://github.com/slowli/tracing-toolbox/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue)](https://github.com/slowli/tracing-toolbox#license)
![rust 1.60+ required](https://img.shields.io/badge/rust-1.60+-blue.svg?label=Required%20Rust)

**Documentation:**
[![crate docs (main)](https://img.shields.io/badge/main-yellow.svg?label=docs)](https://slowli.github.io/tracing-toolbox/tracing_tunnel/)

This crate provides [tracing] infrastructure helpers allowing to transfer tracing events
across an API boundary:

- `TracingEventSender` is a tracing [`Subscriber`] that converts tracing events
  into (de)serializable presentation that can be sent elsewhere using a customizable hook.
- `TracingEventReceiver` consumes events produced by a `TracingEventSender` and relays them
  to the tracing infrastructure. It is assumed that the source of events may outlive
  both the lifetime of a particular `TracingEventReceiver` instance, and the lifetime
  of the program encapsulating the receiver. To deal with this, the receiver provides
  the means to persist / restore its state.

This solves the problem of having *dynamic* call sites for tracing spans / events, 
i.e., ones not known during compilation. This may occur if call sites
are defined in dynamically loaded modules, the execution of which is embedded into the program,
e.g., WASM modules.

See the crate docs for the details about the crate design and potential use cases.

## Usage

Add this to your `Crate.toml`:

```toml
[dependencies]
tracing-tunnel = "0.1.0"
```

Note that the both pieces of functionality described above are gated behind opt-in features;
consult the crate docs for details.

### Sending tracing events

```rust
use std::sync::mpsc;
use tracing_tunnel::{TracingEvent, TracingEventSender, TracingEventReceiver};

// Let's collect tracing events using an MPSC channel.
let (events_sx, events_rx) = mpsc::sync_channel(10);
let subscriber = TracingEventSender::new(move |event| {
    events_sx.send(event).ok();
});

tracing::subscriber::with_default(subscriber, || {
    tracing::info_span!("test", num = 42_i64).in_scope(|| {
        tracing::warn!("I feel disturbance in the Force...");
    });
});

let events: Vec<TracingEvent> = events_rx.iter().collect();
println!("{events:?}");
// Do something with events...
```

### Receiving tracing events

```rust
use std::sync::mpsc;
use tracing_tunnel::{PersistedMetadata, TracingEvent, TracingEventReceiver};

tracing_subscriber::fmt().pretty().init();

fn replay_events(events: &[TracingEvent]) {
    let mut receiver = TracingEventReceiver::default();
    for event in events {
        if let Err(err) = receiver.try_receive(event.clone()) {
            tracing::warn!(%err, "received invalid tracing event");
        }
    }

    // Persist the resulting receiver state. There are two pieces
    // of the state: metadata and alive spans.
    let mut metadata = PersistedMetadata::default();
    receiver.persist_metadata(&mut metadata);
    let spans = receiver.persist_spans(); 
    // Store `metadata` and `spans`, e.g., in a DB
}
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `tracing-toolbox` by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[`tardigrade`]: https://crates.io/crates/tardigrade
[tracing]: https://docs.rs/tracing/0.1/tracing
[`Subscriber`]: https://docs.rs/tracing-core/0.1/tracing_core/trait.Subscriber.html
[The Tardigrade runtime]: https://crates.io/crates/tardigrade-rt
