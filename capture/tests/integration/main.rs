//! Integration tests for tracing capture.

use assert_matches::assert_matches;
use tracing_core::{Level, LevelFilter};
use tracing_subscriber::{layer::SubscriberExt, Registry};

use std::borrow::Cow;

mod fib;

use tracing_capture::{CaptureLayer, SharedStorage, Storage};
use tracing_tunnel::{
    CallSiteData, CallSiteKind, LocalSpans, PersistedMetadata, PersistedSpans, TracedValue,
    TracedValues, TracingEvent, TracingEventReceiver, TracingLevel,
};

const CALL_SITE_DATA: CallSiteData = CallSiteData {
    kind: CallSiteKind::Span,
    name: Cow::Borrowed("test"),
    target: Cow::Borrowed("tracing_tunnel"),
    level: TracingLevel::Error,
    module_path: Some(Cow::Borrowed("integration")),
    file: Some(Cow::Borrowed("tests")),
    line: Some(42),
    fields: Vec::new(),
};

// This really tests `TracingEventReceiver`, but we cannot place the test in `tracing-tunnel`
// because this would create a circular dependency.
#[test]
fn replayed_spans_are_closed_if_entered_multiple_times() {
    let events = [
        TracingEvent::NewCallSite {
            id: 0,
            data: CALL_SITE_DATA,
        },
        TracingEvent::NewSpan {
            id: 0,
            parent_id: None,
            metadata_id: 0,
            values: TracedValues::new(),
        },
        TracingEvent::SpanEntered { id: 0 },
        TracingEvent::SpanExited { id: 0 },
        TracingEvent::SpanEntered { id: 0 },
        TracingEvent::SpanExited { id: 0 },
        TracingEvent::SpanDropped { id: 0 },
    ];

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || {
        let mut spans = PersistedSpans::default();
        let mut local_spans = LocalSpans::default();
        let mut receiver =
            TracingEventReceiver::new(PersistedMetadata::default(), &mut spans, &mut local_spans);
        for event in events {
            receiver.receive(event);
        }
    });

    let storage = storage.lock();
    let span = &storage.spans()[0];
    assert_eq!(span.stats().entered, 2);
    assert_eq!(span.stats().exited, 2);
    assert!(span.stats().is_closed);
}

#[test]
fn capturing_spans_directly() {
    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || fib::fib(5));

    assert_captured_spans(&storage.lock());
}

fn assert_captured_spans(storage: &Storage) {
    let fib_span = storage
        .spans()
        .iter()
        .find(|span| span.metadata().name() == "compute")
        .unwrap();
    assert_eq!(fib_span.metadata().target(), "fib");
    assert_eq!(fib_span.stats().entered, 1);
    assert!(fib_span.stats().is_closed);
    assert_matches!(fib_span["count"], TracedValue::UInt(5));

    assert_eq!(fib_span.events().len(), 6); // 5 iterations + return
    let iter_events = fib_span.events()[0..5].iter();
    for (i, event) in iter_events.enumerate() {
        assert_eq!(event.metadata().target(), "fib");
        assert_eq!(*event.metadata().level(), Level::DEBUG);
        assert_eq!(
            event["message"].as_debug_str(),
            Some("performing iteration")
        );
        assert_eq!(event["i"], i as u64);
    }
    let return_event = fib_span.events().last().unwrap();
    assert_eq!(*return_event.metadata().level(), Level::INFO);
    assert!(return_event["return"].is_debug(&5));

    let outer_span = &storage.spans()[0];
    assert_eq!(outer_span.metadata().name(), "fib");
    assert_eq!(outer_span["approx"], 5.0_f64);
    assert_eq!(outer_span.events().len(), 2);
    let warn_event = &outer_span.events()[0];
    assert_eq!(*warn_event.metadata().level(), Level::WARN);
    assert_eq!(
        warn_event["message"].as_debug_str(),
        Some("count looks somewhat large")
    );
}

#[test]
fn capturing_spans_for_replayed_events() {
    let events = fib::record_events(5);

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || {
        let mut spans = PersistedSpans::default();
        let mut local_spans = LocalSpans::default();
        let mut consumer =
            TracingEventReceiver::new(PersistedMetadata::default(), &mut spans, &mut local_spans);
        for event in events {
            consumer.receive(event.clone());
        }
    });

    assert_captured_spans(&storage.lock());
}

#[test]
fn capturing_events_with_indirect_ancestor() {
    #[tracing::instrument(level = "debug", ret)]
    fn double(value: i32) -> i32 {
        tracing::info!(value, "doubled");
        value * 2
    }

    let storage = SharedStorage::default();
    let layer = CaptureLayer::new(&storage).with_filter(LevelFilter::INFO);
    let subscriber = Registry::default().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!("wrapper").in_scope(|| double(5));
        // The event in this span is captured as a root event.
        tracing::debug_span!("debug_wrapper").in_scope(|| double(-3));
    });

    let storage = storage.lock();
    assert_eq!(storage.spans().len(), 1);
    assert_eq!(storage.all_events().count(), 2);
    let span_events = storage.spans()[0].events();
    assert_eq!(span_events.len(), 1);
    assert!(span_events[0].value("message").is_some());
    assert_eq!(span_events[0]["value"], 5_i64);
    let root_events = storage.root_events();
    assert_eq!(root_events.len(), 1);
    assert_eq!(root_events[0]["value"], -3_i64);
}
