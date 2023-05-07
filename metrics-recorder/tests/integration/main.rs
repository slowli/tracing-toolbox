use metrics::Unit;
use tracing::{Level, Subscriber};
use tracing_subscriber::{layer::SubscriberExt, registry::LookupSpan, FmtSubscriber};

use std::{thread, time::Duration};

mod multithreaded;

use tracing_capture::{metrics::MetricKind, CaptureLayer, SharedStorage};
use tracing_metrics_recorder::TracingMetricsRecorder;

fn create_fmt_subscriber() -> impl Subscriber + for<'a> LookupSpan<'a> {
    FmtSubscriber::builder()
        .pretty()
        .with_max_level(Level::TRACE)
        .with_test_writer()
        .finish()
}

fn generate_metrics() {
    metrics::describe_histogram!(
        "greeting.latency",
        Unit::Seconds,
        "time spent on a greeting"
    );

    tracing::info_span!("greeting").in_scope(|| {
        for i in 0..5 {
            let _entered = tracing::info_span!("iteration", i).entered();
            metrics::counter!("greeting.count", 1);
            metrics::gauge!("greeting.oddity", (i % 2) as f64);

            thread::sleep(Duration::from_millis(10));
            metrics::histogram!(
                "greeting.latency",
                Duration::from_millis(10),
                "oddity" => (i % 2).to_string()
            );
        }
    });
}

fn test_recording_metrics_as_events() {
    let _guard = TracingMetricsRecorder::set().unwrap();
    let storage = SharedStorage::default();
    let subscriber = create_fmt_subscriber().with(CaptureLayer::new(&storage));

    tracing::subscriber::with_default(subscriber, generate_metrics);

    let storage = storage.lock();
    let counter_updates = storage.all_events().filter_map(|event| {
        let update = event.as_metric_update()?;
        // Check that all metric updates are properly located in the span tree.
        let parent_span = event.parent().unwrap();
        assert_eq!(parent_span.metadata().name(), "iteration");
        assert!(parent_span["i"].as_int().is_some());

        if update.metric.name == "greeting.count" {
            assert_eq!(update.metric.kind, MetricKind::Counter);
            assert!(update.metric.labels.is_empty());
            return Some((update.prev_value.as_uint()?, update.value.as_uint()?));
        }
        None
    });
    let counter_updates: Vec<_> = counter_updates.collect();
    assert_eq!(counter_updates, [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]);

    let gauge_updates = storage.all_events().filter_map(|event| {
        let update = event.as_metric_update()?;
        if update.metric.name == "greeting.oddity" {
            assert_eq!(update.metric.kind, MetricKind::Gauge);
            return update.value.as_float();
        }
        None
    });
    let gauge_updates: Vec<_> = gauge_updates.collect();
    assert_eq!(gauge_updates, [0.0, 1.0, 0.0, 1.0, 0.0]);

    let histogram_updates = storage.all_events().filter(|event| {
        if let Some(update) = event.as_metric_update() {
            if update.metric.name == "greeting.latency" {
                assert_eq!(update.metric.kind, MetricKind::Histogram);
                assert!(["0", "1"].contains(&update.metric.labels["oddity"]));
                assert_eq!(update.metric.unit, "seconds");
                let latency = update.value.as_float().unwrap();
                assert!(latency > 0.0 && latency < 1.0);
                return true;
            }
        }
        false
    });
    assert_eq!(histogram_updates.count(), 5);
}

#[test]
fn recording_metrics_as_events() {
    test_recording_metrics_as_events();
    test_recording_metrics_as_events(); // Check that metrics are cleaned up
}

#[test]
fn delayed_recording_metrics_as_events() {
    thread::sleep(Duration::from_millis(500)); // Wait until the first test is done
    test_recording_metrics_as_events();
}
