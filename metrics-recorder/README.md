# Tracing Metrics Recorder

[![Build Status](https://github.com/slowli/tracing-toolbox/workflows/CI/badge.svg?branch=main)](https://github.com/slowli/tracing-toolbox/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue)](https://github.com/slowli/tracing-toolbox#license)
![rust 1.64+ required](https://img.shields.io/badge/rust-1.64+-blue.svg?label=Required%20Rust)

**Documentation:**
[![crate docs (main)](https://img.shields.io/badge/main-yellow.svg?label=docs)](https://slowli.github.io/tracing-toolbox/tracing_metrics_recorder/)

This crate provides a [`metrics`] recorder that emits [`tracing`] events
each time a metric is updated. This can be used for debugging
(the metrics can be logged to stdout / stderr while retaining contextual information
about tracing spans), or for testing (the emitted events can be [captured][`tracing-capture`]
and asserted against).

## Usage

Add this to your `Crate.toml`:

```toml
[dependencies]
tracing-metrics-recorder = "0.1.0"
```

### Using recorder for testing

```rust
use metrics::Unit;
use tracing_capture::{CaptureLayer, SharedStorage};
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tracing_metrics_recorder::TracingMetricsRecorder;
use std::{error::Error, thread, time::{Duration, Instant}};

// Install a metrics recorder and a capturing metrics subscriber
// for the current thread.
let _guard = TracingMetricsRecorder::set()?;
let storage = SharedStorage::default();
let subscriber = Registry::default().with(CaptureLayer::new(&storage));
let _guard = tracing::subscriber::set_default(subscriber);

// Execute code with metrics.
metrics::describe_histogram!("latency", Unit::Seconds, "Cycle latency");
for i in 0..5 {
    let start = Instant::now();
    thread::sleep(Duration::from_millis(10));
    metrics::histogram!("latency", start.elapsed());
}

// Check that metrics have been collected.
let storage = storage.lock();
let histogram_values = storage.all_events().filter_map(|event| {
    let event = event.as_metric_update()?;
    if event.metric.name == "latency" {
        assert_eq!(event.metric.unit, "seconds");
        assert_eq!(event.metric.description, "Cycle latency");
        return event.value.as_float();
    }
    None
});
let histogram_values: Vec<_> = histogram_values.collect();
assert_eq!(histogram_values.len(), 5);
println!("{histogram_values:?}");
Ok::<_, Box<dyn Error>>(())
```

See crate docs for the specification of emitted events and more examples.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `tracing-toolbox` by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[`metrics`]: https://crates.io/crates/metrics
[`tracing`]: https://crates.io/crates/tracing
[`tracing-capture`]: https://crates.io/crates/tracing-capture
