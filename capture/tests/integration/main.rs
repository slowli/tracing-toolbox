//! Integration tests for tracing capture.

use assert_matches::assert_matches;
use predicates::ord::eq;
use tracing_core::{Level, LevelFilter};
use tracing_subscriber::{layer::SubscriberExt, Registry};

use std::borrow::Cow;

mod fib;

use tracing_capture::{
    predicates::{ancestor, field, level, message, name, parent, ScanExt},
    CaptureLayer, SharedStorage, Storage,
};
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
    let span = storage.all_spans().next().unwrap();
    assert_eq!(span.stats().entered, 2);
    assert_eq!(span.stats().exited, 2);
    assert!(span.stats().is_closed);
}

// This is also a `TracingEventReceiver` test.
#[test]
fn recorded_span_values_are_restored() {
    let events = [
        TracingEvent::NewCallSite {
            id: 0,
            data: CallSiteData {
                fields: vec!["i".into()],
                ..CALL_SITE_DATA
            },
        },
        TracingEvent::NewSpan {
            id: 0,
            parent_id: None,
            metadata_id: 0,
            values: TracedValues::from_iter([("i".to_owned(), TracedValue::from(42_i64))]),
        },
        TracingEvent::SpanEntered { id: 0 },
        TracingEvent::SpanExited { id: 0 },
    ];

    let mut spans = PersistedSpans::default();
    let mut local_spans = LocalSpans::default();
    let mut receiver =
        TracingEventReceiver::new(PersistedMetadata::default(), &mut spans, &mut local_spans);
    for event in events {
        receiver.receive(event);
    }
    let mut metadata = PersistedMetadata::default();
    receiver.persist_metadata(&mut metadata);

    // Emulate host restart: persisted metadata / spans are restored, but `local_spans` are not.
    let more_events = [
        TracingEvent::SpanEntered { id: 0 },
        TracingEvent::NewCallSite {
            id: 1,
            data: CallSiteData {
                fields: vec!["message".into()],
                ..CALL_SITE_DATA
            },
        },
        TracingEvent::NewEvent {
            metadata_id: 1,
            parent: None,
            values: TracedValues::from_iter([("message".to_owned(), TracedValue::from("test"))]),
        },
        TracingEvent::SpanExited { id: 0 },
        TracingEvent::SpanDropped { id: 0 },
    ];
    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    let mut local_spans = LocalSpans::default();
    tracing::subscriber::with_default(subscriber, || {
        let mut receiver = TracingEventReceiver::new(metadata, &mut spans, &mut local_spans);
        for event in more_events {
            receiver.receive(event);
        }
    });

    let storage = storage.lock();
    let span = storage.all_spans().next().unwrap();
    assert_eq!(span["i"], 42_i64);
    assert_eq!(span.stats().entered, 1);
    assert!(span.stats().is_closed);
    let event = span.events().next().unwrap();
    assert_eq!(event["message"].as_str(), Some("test"));
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
        .all_spans()
        .find(|span| span.metadata().name() == "compute")
        .unwrap();
    assert_eq!(fib_span.metadata().target(), "fib");
    assert_eq!(fib_span.stats().entered, 1);
    assert!(fib_span.stats().is_closed);
    assert_matches!(fib_span["count"], TracedValue::UInt(5));

    assert_eq!(fib_span.events().len(), 6); // 5 iterations + return
    let iter_events = fib_span.events().take(5);
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

    let outer_span = storage.all_spans().next().unwrap();
    assert_eq!(outer_span.metadata().name(), "fib");
    assert_eq!(outer_span["approx"], 5.0_f64);
    assert_eq!(outer_span.events().len(), 2);
    let warn_event = outer_span.events().next().unwrap();
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
    assert_eq!(storage.all_spans().len(), 1);
    assert_eq!(storage.all_events().len(), 2);

    let span = storage.all_spans().next().unwrap();
    let mut span_events = span.events();
    assert_eq!(span_events.len(), 1);
    let span_event = span_events.next().unwrap();
    assert!(span_event.value("message").is_some());
    assert_eq!(span_event["value"], 5_i64);

    let mut root_events = storage.root_events();
    assert_eq!(root_events.len(), 1);
    let root_event = root_events.next().unwrap();
    assert_eq!(root_event["value"], -3_i64);

    let predicate = message(eq("doubled")) & parent(level(Level::INFO) & name(eq("wrapper")));
    let span_event = storage.scan_events().single(&predicate);
    assert_eq!(span_event["value"], 5_i64);
}

#[test]
fn capturing_span_hierarchy() {
    #[tracing::instrument(level = "debug", ret)]
    fn factorial(value: u32) -> u32 {
        tracing::info!(value, "doubled");
        if value == 0 {
            1
        } else {
            value * factorial(value - 1)
        }
    }

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || factorial(5));

    let storage = storage.lock();
    assert_eq!(storage.all_spans().len(), 6);
    assert_eq!(storage.all_events().len(), 12);
    assert_eq!(storage.root_spans().len(), 1);
    assert_eq!(storage.root_events().len(), 0);

    let inner_span = storage.all_spans().rev().next().unwrap();
    assert_eq!(inner_span["value"], 0_u64);
    let ancestor_values: Vec<_> = inner_span
        .ancestors()
        .filter_map(|span| span["value"].as_uint())
        .collect();
    assert_eq!(ancestor_values, [1, 2, 3, 4, 5]);

    let middle_span = storage.scan_spans().single(&field("value", 3_u64));
    let ancestor_values: Vec<_> = middle_span
        .ancestors()
        .filter_map(|span| span["value"].as_uint())
        .collect();
    assert_eq!(ancestor_values, [4, 5]);

    let event_filter = parent(field("value", 3_u64)) & message(eq("doubled"));
    storage.scan_events().single(&event_filter);
    let event_filter = field("value", 2_u64) & ancestor(field("value", 3_u64));
    storage.scan_events().single(&event_filter);
}

#[test]
fn capturing_wide_span_graph() {
    const MAX_DEPTH: usize = 3;

    #[tracing::instrument(level = "debug")]
    fn graph(counter: &mut u64, depth: usize) {
        *counter += 1;
        if depth == MAX_DEPTH {
            tracing::debug!(depth, "reached max depth");
        } else {
            let children_count = if *counter % 2 == 0 { 2 } else { 3 };
            for _ in 0..children_count {
                graph(counter, depth + 1);
            }
        }
    }

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || graph(&mut 0, 0));

    let storage = storage.lock();
    assert_eq!(storage.root_spans().len(), 1);
    let root = storage.root_spans().next().unwrap();
    let counters: Vec<_> = root
        .descendants()
        .filter_map(|span| span["counter"].as_uint())
        .collect();
    let max = counters.len() as u128;
    assert!(counters.iter().copied().eq(1..=max), "{counters:?}");

    let predicate = level(Level::DEBUG) & field("counter", 10_u64);
    root.deep_scan_spans().single(&predicate);
    let predicate = field("depth", 3_u64) & message(eq("reached max depth"));
    let event = root.deep_scan_events().first(&predicate);
    let ancestor_counters: Vec<_> = event
        .ancestors()
        .filter_map(|span| span["counter"].as_uint())
        .collect();
    assert_eq!(ancestor_counters, [3, 2, 1, 0]);
}
