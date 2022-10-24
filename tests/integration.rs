//! Integration tests for Tardigrade tracing infrastructure.

use assert_matches::assert_matches;
use insta::assert_yaml_snapshot;
use once_cell::sync::Lazy;
use tracing_core::{field, Level, Subscriber};
use tracing_subscriber::{layer::SubscriberExt, registry::LookupSpan, FmtSubscriber, Registry};

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    error, fmt,
    sync::{Arc, Mutex},
};

use tardigrade_tracing::{
    capture::{CaptureLayer, SharedStorage, Storage},
    CallSiteKind, EmittingSubscriber, EventConsumer, PersistedMetadata, PersistedSpans,
    TracedValue, TracingEvent, TracingLevel,
};

#[derive(Debug)]
struct Overflow;

impl fmt::Display for Overflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "integer overflow")
    }
}

impl error::Error for Overflow {}

#[tracing::instrument(target = "fib", ret, err)]
fn compute(count: usize) -> Result<u64, Overflow> {
    let (mut x, mut y) = (0_u64, 1_u64);
    for i in 0..count {
        tracing::debug!(target: "fib", i, current = x, "performing iteration");
        (x, y) = (y, x.checked_add(y).ok_or(Overflow)?);
    }
    Ok(x)
}

const PHI: f64 = 1.618033988749895; // (1 + sqrt(5)) / 2

fn fib(count: usize) {
    let span = tracing::info_span!("fib", approx = field::Empty);
    let _entered = span.enter();

    let approx = PHI.powi(count as i32) / 5.0_f64.sqrt();
    let approx = approx.round();
    span.record("approx", approx);

    tracing::warn!(count, "count looks somewhat large");
    match compute(count) {
        Ok(result) => {
            tracing::info!(result, "computed Fibonacci number");
        }
        Err(err) => {
            tracing::error!(error = &err as &dyn error::Error, "computation failed");
        }
    }
}

/// **NB.** Must be called once per program run; otherwise, call sites will be missing
/// on subsequent runs.
fn record_events(count: usize) -> Vec<TracingEvent> {
    let events = Arc::new(Mutex::new(vec![]));
    let events_ = Arc::clone(&events);
    let recorder = EmittingSubscriber::new(move |event| {
        events_.lock().unwrap().push(event);
    });

    tracing::subscriber::with_default(recorder, || fib(count));
    Arc::try_unwrap(events).unwrap().into_inner().unwrap()
}

#[derive(Debug)]
struct RecordedEvents {
    short: Vec<TracingEvent>,
    long: Vec<TracingEvent>,
}

static EVENTS: Lazy<RecordedEvents> = Lazy::new(|| RecordedEvents {
    short: record_events(5),
    long: record_events(80),
});

#[test]
fn event_snapshot() {
    let mut events = EVENTS.short.clone();
    for event in &mut events {
        if let TracingEvent::NewCallSite { data, .. } = event {
            // Make event data not depend on specific lines, which could easily
            // change due to refactoring etc.
            data.line = Some(42);
            if matches!(data.kind, CallSiteKind::Event) {
                data.name = Cow::Borrowed("event");
            }
        }
    }
    assert_yaml_snapshot!("events-fib-5", events);
}

#[test]
fn resource_management_for_tracing_events() {
    let events = &EVENTS.long;

    let mut alive_spans = HashSet::new();
    let mut open_spans = vec![];
    for event in events {
        match event {
            TracingEvent::NewSpan { id, .. } => {
                assert!(alive_spans.insert(*id));
            }
            TracingEvent::SpanCloned { .. } => unreachable!(),
            TracingEvent::SpanDropped { id } => {
                assert!(!open_spans.contains(id));
                assert!(alive_spans.remove(id));
            }

            TracingEvent::SpanEntered { id } => {
                assert!(alive_spans.contains(id));
                assert!(!open_spans.contains(id));
                open_spans.push(*id);
            }
            TracingEvent::SpanExited { id } => {
                assert!(alive_spans.contains(id));
                let popped_span = open_spans.pop();
                assert_eq!(popped_span, Some(*id));
            }

            _ => { /* Do nothing */ }
        }
    }
    assert!(alive_spans.is_empty());
    assert!(open_spans.is_empty());
}

#[test]
fn call_sites_for_tracing_events() {
    let events = &EVENTS.long;

    let fields_by_span = events.iter().filter_map(|event| {
        if let TracingEvent::NewCallSite { data, .. } = event {
            if matches!(data.kind, CallSiteKind::Span) {
                let fields: Vec<_> = data.fields.iter().map(Cow::as_ref).collect();
                return Some((data.name.as_ref(), fields));
            }
        }
        None
    });
    let fields_by_span: HashMap<_, _> = fields_by_span.collect();
    assert_eq!(fields_by_span.len(), 2);
    assert_eq!(fields_by_span["fib"], ["approx"]);
    assert_eq!(fields_by_span["compute"], ["count"]);

    let mut known_metadata_ids = HashSet::new();
    let event_call_sites: Vec<_> = events
        .iter()
        .filter_map(|event| {
            if let TracingEvent::NewCallSite { id, data } = event {
                assert!(known_metadata_ids.insert(*id));
                if matches!(data.kind, CallSiteKind::Event) {
                    return Some(data);
                }
            }
            None
        })
        .collect();

    let targets: HashSet<_> = event_call_sites
        .iter()
        .map(|site| site.target.as_ref())
        .collect();
    assert_eq!(targets, HashSet::from_iter(["fib", "integration"]));

    let mut call_sites_by_level = HashMap::<_, usize>::new();
    for site in &event_call_sites {
        *call_sites_by_level.entry(site.level).or_default() += 1;
    }
    assert_eq!(call_sites_by_level[&TracingLevel::Warn], 1);
    assert_eq!(call_sites_by_level[&TracingLevel::Info], 2);
    assert_eq!(call_sites_by_level[&TracingLevel::Debug], 1);
}

#[test]
fn event_fields_have_same_order() {
    let events = &EVENTS.long;

    let debug_metadata_id = events.iter().find_map(|event| {
        if let TracingEvent::NewCallSite { id, data } = event {
            if matches!(data.kind, CallSiteKind::Event) && data.level == TracingLevel::Debug {
                return Some(*id);
            }
        }
        None
    });
    let debug_metadata_id = debug_metadata_id.unwrap();

    let debug_fields = events.iter().filter_map(|event| {
        if let TracingEvent::NewEvent {
            metadata_id,
            values,
            ..
        } = event
        {
            if *metadata_id == debug_metadata_id {
                return Some(values.as_slice());
            }
        }
        None
    });

    for fields in debug_fields {
        assert_matches!(
            fields,
            [
                (message, TracedValue::Object(_)),
                (i, TracedValue::UInt(_)),
                (current, TracedValue::UInt(_)),
            ] if i == "i" && current == "current" && message == "message"
        );
    }
}

fn create_fmt_subscriber() -> impl Subscriber + for<'a> LookupSpan<'a> {
    FmtSubscriber::builder()
        .pretty()
        .with_max_level(Level::TRACE)
        .with_test_writer()
        .finish()
}

/// This test are mostly about the "expected" output of `FmtSubscriber`.
/// Their output should be reviewed manually.
#[test]
fn reproducing_events_on_fmt_subscriber() {
    let events = &EVENTS.long;

    let mut consumer = EventConsumer::default();
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        for event in events {
            consumer.consume_event(event.clone());
        }
    });
}

#[test]
fn persisting_metadata() {
    let events = &EVENTS.short;

    let mut persisted = PersistedMetadata::default();
    let mut consumer = EventConsumer::new(&mut persisted, &mut PersistedSpans::default());
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        for event in events {
            consumer.consume_event(event.clone());
        }
    });
    consumer.persist_metadata(&mut persisted);

    let names: HashSet<_> = persisted
        .iter()
        .map(|(_, data)| data.name.as_ref())
        .collect();
    assert!(names.contains("fib"), "{names:?}");
    assert!(names.contains("compute"), "{names:?}");

    // Check that `consumer` can function after restoring `persisted` meta.
    let mut consumer = EventConsumer::new(&mut persisted, &mut PersistedSpans::default());
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        for event in events {
            if !matches!(event, TracingEvent::NewCallSite { .. }) {
                consumer.consume_event(event.clone());
            }
        }
    });
}

#[test]
fn persisting_spans() {
    let events = &EVENTS.short;

    let mut metadata = PersistedMetadata::default();
    let mut spans = PersistedSpans::default();
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        let mut consumer = EventConsumer::new(&mut metadata, &mut spans);
        for event in events {
            consumer.consume_event(event.clone());

            if matches!(
                event,
                TracingEvent::NewSpan { .. } | TracingEvent::SpanExited { .. }
            ) {
                // Emulate consumer reset. When the matched events are emitted,
                // spans should be non-empty.
                consumer.persist_metadata(&mut metadata);
                spans = consumer.persist_spans();
                consumer = EventConsumer::new(&mut metadata, &mut spans);
            }
        }
    });
}

#[test]
fn capturing_spans_directly() {
    Lazy::force(&EVENTS); // necessary to not influence the `EmittingSubscriber`

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || fib(5));

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
    let events = &EVENTS.short;

    let storage = SharedStorage::default();
    let subscriber = Registry::default().with(CaptureLayer::new(&storage));
    tracing::subscriber::with_default(subscriber, || {
        let mut consumer = EventConsumer::default();
        for event in events {
            consumer.consume_event(event.clone());
        }
    });

    assert_captured_spans(&storage.lock());
}
