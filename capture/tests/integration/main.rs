//! Integration tests for tracing capture.

use std::{borrow::Cow, panic, thread, time::Duration};

use assert_matches::assert_matches;
use predicates::ord::eq;
use tracing_capture::{
    predicates::{ancestor, field, level, message, name, parent, ScanExt},
    CaptureLayer, SharedStorage, Storage,
};
use tracing_core::{Level, LevelFilter};
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tracing_tunnel::{
    CallSiteData, CallSiteKind, LocalSpans, TracedValue, TracedValues, TracingEvent,
    TracingEventReceiver, TracingLevel,
};

mod fib;

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
        let mut receiver = TracingEventReceiver::default();
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

    let mut receiver = TracingEventReceiver::default();
    for event in events {
        receiver.receive(event);
    }
    let metadata = receiver.persist_metadata();
    let (spans, _) = receiver.persist();

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
    tracing::subscriber::with_default(subscriber, || {
        let mut receiver = TracingEventReceiver::new(metadata, spans, LocalSpans::default());
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

// This is also a `TracingEventReceiver` test.
#[test]
fn spans_are_exited_on_receiver_drop() {
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
            values: TracedValues::new(),
        },
        TracingEvent::SpanEntered { id: 0 },
    ];

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    let _guard = tracing::subscriber::set_default(subscriber);

    let mut receiver = TracingEventReceiver::default();
    for event in events {
        receiver.receive(event);
    }
    let metadata = receiver.persist_metadata();
    let (spans, local_spans) = receiver.persist();

    {
        let storage = storage.lock();
        let span = storage.all_spans().next().unwrap();
        assert_eq!(span.stats().entered, 1);
        assert_eq!(span.stats().exited, 1); // <<< force-exited on receiver drop
        assert!(!span.stats().is_closed);
    }

    let more_events = [
        TracingEvent::NewSpan {
            id: 1,
            parent_id: None,
            metadata_id: 0,
            values: TracedValues::new(),
        },
        TracingEvent::SpanEntered { id: 1 },
        TracingEvent::SpanEntered { id: 0 },
    ];
    let mut receiver = TracingEventReceiver::new(metadata, spans, local_spans);
    for event in more_events {
        receiver.receive(event);
    }
    drop(receiver); // discard the execution

    let storage = storage.lock();
    let spans: Vec<_> = storage.all_spans().collect();
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert_eq!(spans[0].stats().entered, 2);
    assert_eq!(spans[0].stats().exited, 2); // <<< force-exited on receiver drop
    assert!(!spans[0].stats().is_closed);
    assert_eq!(spans[1].stats().entered, 1);
    assert_eq!(spans[1].stats().exited, 1); // <<< force-exited on receiver drop
    assert!(spans[1].stats().is_closed);
    // ^ auto-closed since the span is created by the discarded execution
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
        assert_eq!(event.message(), Some("performing iteration"));
        assert_eq!(event["i"], i as u64);
    }
    let return_event = fib_span.events().next_back().unwrap();
    assert_eq!(*return_event.metadata().level(), Level::INFO);
    assert!(return_event["return"].is_debug(&5));

    let outer_span = storage.all_spans().next().unwrap();
    assert_eq!(outer_span.metadata().name(), "fib");
    assert_eq!(outer_span["approx"], 5.0_f64);
    assert_eq!(outer_span.events().len(), 2);
    let warn_event = outer_span.events().next().unwrap();
    assert_eq!(*warn_event.metadata().level(), Level::WARN);
    assert_eq!(warn_event.message(), Some("count looks somewhat large"));
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

    assert!(span.descendant_events().next().is_none());
    span.deep_scan_events().first(&field("value", 5_i64));

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

    let inner_span = storage.all_spans().next_back().unwrap();
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

    // Check that ordering of spans / events works as advertised.
    let span_pairs = storage.all_spans().zip(storage.all_spans().skip(1));
    for (prev, next) in span_pairs {
        assert!(prev < next);
    }
    for span in storage.all_spans() {
        for child in span.children() {
            assert!(span < child);
        }
        assert!(span.follows_from().next().is_none());
    }
    let event_pairs = storage.all_events().zip(storage.all_events().skip(1));
    for (prev, next) in event_pairs {
        assert!(prev < next);
    }

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

#[test]
fn items_from_different_storages_are_not_comparable() {
    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn_span!("greeting").in_scope(|| {
            tracing::info!("hello, world!");
        });
    });

    let other_storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&other_storage));
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn_span!("greeting").in_scope(|| {
            tracing::info!("hello, world!");
        });
    });

    let storage = storage.lock();
    let span = storage.root_spans().next().unwrap();
    let event = storage.all_events().next().unwrap();
    let other_storage = other_storage.lock();
    let other_span = other_storage.root_spans().next().unwrap();
    let other_event = other_storage.all_events().next().unwrap();

    assert_eq!(span, span);
    assert_eq!(storage.all_spans().next(), Some(span));
    assert_ne!(span, other_span);
    assert!(span.partial_cmp(&other_span).is_none());

    assert_eq!(event, event);
    assert_eq!(span.events().next(), Some(event));
    assert_ne!(event, other_event);
    assert!(event.partial_cmp(&other_event).is_none());
}

#[test]
fn explicit_parent_is_correctly_handled() {
    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || {
        let parent = tracing::warn_span!("greeting");
        tracing::info_span!("_").in_scope(|| {
            tracing::info!(parent: &parent, "hello, world!");
            tracing::info_span!(parent: &parent, "child").in_scope(|| {
                tracing::info!("hi");
            });
        });
    });

    let storage = storage.lock();
    let mut events = storage.all_events();
    assert_eq!(events.len(), 2);
    let event = events.next().unwrap();
    let event_parent = event.parent().unwrap();
    assert_eq!(event_parent.metadata().name(), "greeting");

    let child_span = events.next().unwrap().parent().unwrap();
    assert_eq!(child_span.metadata().name(), "child");
    assert_eq!(child_span.parent(), Some(event_parent));
}

#[test]
fn recording_follows_from_relations() {
    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || {
        let main_span = tracing::info_span!("main");
        let other_span = tracing::info_span!("other");
        for i in 0..3 {
            let task_span = tracing::debug_span!("task", i);
            task_span.follows_from(&main_span);
            if i == 0 {
                task_span.follows_from(&other_span);
            }

            let _entered = task_span.enter();
            thread::sleep(Duration::from_millis(5));
            tracing::info!(i, "task finished");
        }
    });

    let storage = storage.lock();
    assert_eq!(storage.root_spans().len(), 5);

    let task_spans = storage
        .root_spans()
        .filter(|span| *span.metadata().level() == Level::DEBUG);
    for (i, task_span) in task_spans.enumerate() {
        assert_eq!(task_span.metadata().name(), "task");
        assert_eq!(task_span.follows_from().len(), if i == 0 { 2 } else { 1 });
        let mut followed_spans = task_span.follows_from();
        let followed_span = followed_spans.next().unwrap();
        assert_eq!(*followed_span.metadata().level(), Level::INFO);
        assert_eq!(followed_span.metadata().name(), "main");
        if i == 0 {
            let followed_span = followed_spans.next().unwrap();
            assert_eq!(*followed_span.metadata().level(), Level::INFO);
            assert_eq!(followed_span.metadata().name(), "other");
        }
    }
}

#[test]
fn failed_assertion_while_storage_is_locked() {
    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    let _guard = tracing::subscriber::set_default(subscriber);

    let panic_result = panic::catch_unwind(|| {
        tracing::info_span!("_").in_scope(|| {
            tracing::info!("hello, world!");
            let storage = storage.lock();
            assert_eq!(storage.all_events().len(), 2, "Huh?"); // fails
        });
    });
    let err = panic_result.unwrap_err();
    let err = err.downcast_ref::<String>().unwrap();
    assert!(err.contains("Huh?"), "{err}");

    // Check that the `Storage` is not poisoned.
    let storage = storage.lock();
    assert_eq!(storage.all_events().len(), 1);
}
