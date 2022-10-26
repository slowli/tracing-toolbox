//! Integration tests for tracing capture.

use assert_matches::assert_matches;
use tracing_subscriber::{layer::SubscriberExt, Registry};

use std::borrow::Cow;

mod fib;

use tracing_capture::{CaptureLayer, SharedStorage, Storage};
use tracing_tunnel::{
    CallSiteData, CallSiteKind, TracedValue, TracingEvent, TracingEventReceiver, TracingLevel,
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
            values: vec![],
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
        let mut receiver = TracingEventReceiver::default();
        for event in events {
            receiver.receive(event);
        }
    });

    let storage = storage.lock();
    let span = storage.spans().next().unwrap();
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
        .find(|span| span.metadata().name() == "compute")
        .unwrap();
    assert_eq!(fib_span.metadata().target(), "fib");
    assert_eq!(fib_span.stats().entered, 1);
    assert!(fib_span.stats().is_closed);
    assert_matches!(fib_span["count"], TracedValue::UInt(5));
}

#[test]
fn capturing_spans_for_replayed_events() {
    let events = fib::record_events(5);

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || {
        let mut consumer = TracingEventReceiver::default();
        for event in events {
            consumer.receive(event.clone());
        }
    });

    assert_captured_spans(&storage.lock());
}
