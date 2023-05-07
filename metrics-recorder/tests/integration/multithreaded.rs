//! Testing multithreaded setup for `TracingMetricsRecorder`.

use metrics::Unit;
use serial_test::serial;
use tracing_subscriber::{layer::SubscriberExt, Registry};

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use tracing_capture::{CaptureLayer, SharedStorage, Storage};
use tracing_metrics_recorder::TracingMetricsRecorder;

#[test]
#[serial("multithreaded")]
#[should_panic]
fn panics_do_not_prevent_multithreaded_recorder() {
    let _guard = TracingMetricsRecorder::set_global().unwrap();
    metrics::counter!("spawned.counter", 100); // Check that the counter is reset for other tests
    panic!("oops");
}

#[test]
#[serial("multithreaded")]
fn recorder_in_multithreaded_test() {
    thread::sleep(Duration::from_millis(10));
    // ^ Ensure that the panicking test gets the lock first
    let _guard = TracingMetricsRecorder::set_global().unwrap();

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    let subscriber = Arc::new(subscriber);
    tracing::subscriber::with_default(Arc::clone(&subscriber), || {
        metrics::describe_histogram!("spawned.latency", Unit::Seconds, "latency");

        let start = Instant::now();
        let handle = thread::spawn(|| {
            let _guard = tracing::subscriber::set_default(subscriber);

            let start = Instant::now();
            thread::sleep(Duration::from_millis(10));
            metrics::counter!("spawned.counter", 1);
            metrics::histogram!("spawned.latency", start.elapsed(), "thread" => "child");
        });
        handle.join().unwrap();
        metrics::histogram!("spawned.latency", start.elapsed(), "thread" => "main");
    });

    let storage = storage.lock();
    let threads = storage.all_events().filter_map(|event| {
        let update = event.as_metric_update()?;
        if update.metric.name == "spawned.latency" {
            assert_eq!(update.metric.unit, "seconds");
            assert_eq!(update.metric.description, "latency");
            return Some(update.metric.labels["thread"]);
        }
        None
    });
    let threads: Vec<_> = threads.collect();
    assert_eq!(threads, ["child", "main"]);

    assert_counter(&storage);
}

fn assert_counter(storage: &Storage) {
    for event in storage.all_events() {
        if let Some(update) = event.as_metric_update() {
            if update.metric.name == "spawned.counter" {
                assert_eq!(*update.prev_value, 0_u128);
                assert_eq!(*update.value, 1_u128);
            }
        }
    }
}

#[test]
#[serial("multithreaded")]
fn recorder_in_other_multithreaded_test() {
    thread::sleep(Duration::from_millis(100));
    // ^ Ensure that the panicking test gets the lock first
    let _guard = TracingMetricsRecorder::set_global().unwrap();

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    let subscriber = Arc::new(subscriber);
    tracing::subscriber::with_default(Arc::clone(&subscriber), || {
        metrics::describe_histogram!("spawned.latency", Unit::Microseconds, "latency (us)");

        let start = Instant::now();
        let handle = thread::spawn(|| {
            let _guard = tracing::subscriber::set_default(subscriber);

            let start = Instant::now();
            thread::sleep(Duration::from_millis(10));
            metrics::counter!("spawned.counter", 1);
            metrics::histogram!("spawned.latency", start.elapsed().as_micros() as f64);
        });
        handle.join().unwrap();
        metrics::histogram!("spawned.latency", start.elapsed().as_micros() as f64);
    });

    let storage = storage.lock();
    let latency = storage.all_events().filter_map(|event| {
        let update = event.as_metric_update()?;
        if update.metric.name == "spawned.latency" {
            assert_eq!(update.metric.unit, "microseconds");
            assert_eq!(update.metric.description, "latency (us)");
            let prev_value = update.prev_value.as_float().unwrap();
            let value = update.value.as_float().unwrap();
            return Some((prev_value, value));
        }
        None
    });
    let latency: Vec<_> = latency.collect();
    assert_eq!(latency.len(), 2);
    assert_eq!(latency[0].0, 0.0);
    assert_eq!(latency[1].0, latency[0].1);
    assert!(latency[1].0 > 1_000.0);

    assert_counter(&storage);
}
