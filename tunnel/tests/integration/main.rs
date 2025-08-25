//! Integration tests for Tardigrade tracing infrastructure.

use assert_matches::assert_matches;
use once_cell::sync::Lazy;
use tracing_core::{Level, Subscriber};
use tracing_subscriber::{registry::LookupSpan, FmtSubscriber};

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    iter, thread,
};

mod fib;

use tracing_tunnel::{
    CallSiteKind, LocalSpans, PersistedMetadata, PersistedSpans, TracedValue, TracingEvent,
    TracingEventReceiver, TracingEventSender, TracingLevel,
};

#[derive(Debug)]
struct RecordedEvents {
    short: Vec<TracingEvent>,
    long: Vec<TracingEvent>,
}

// **NB.** Tests calling the `fib` module should block on `EVENTS`; otherwise,
// the snapshot tests may fail because of the differing ordering of `NewCallSite` events.
static EVENTS: Lazy<RecordedEvents> = Lazy::new(|| RecordedEvents {
    short: fib::record_events(5),
    long: fib::record_events(80),
});

#[test]
fn event_snapshot() {
    let mut events = EVENTS.short.clone();
    TracingEvent::normalize(&mut events);

    insta::assert_yaml_snapshot!("events-fib-5", events);
}

#[test]
fn resource_management_for_tracing_events() {
    assert_span_management(&EVENTS.long);
}

fn assert_span_management(events: &[TracingEvent]) {
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
    assert_eq!(targets, HashSet::from_iter(["fib", "integration::fib"]));

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
                return Some(values);
            }
        }
        None
    });

    for fields in debug_fields {
        let fields: Vec<_> = fields.iter().collect();
        assert_matches!(
            fields.as_slice(),
            [
                ("message", TracedValue::Object(_)),
                ("i", TracedValue::UInt(_)),
                ("current", TracedValue::UInt(_)),
            ]
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

    let mut consumer = TracingEventReceiver::default();
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        for event in events {
            consumer.receive(event.clone());
        }
    });
}

#[test]
fn persisting_metadata() {
    let events = &EVENTS.short;

    let mut receiver = TracingEventReceiver::default();
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        for event in events {
            receiver.receive(event.clone());
        }
    });
    let metadata = receiver.persist_metadata();
    let (spans, local_spans) = receiver.persist();

    let names: HashSet<_> = metadata
        .iter()
        .map(|(_, data)| data.name.as_ref())
        .collect();
    assert!(names.contains("fib"), "{names:?}");
    assert!(names.contains("compute"), "{names:?}");

    // Check that `receiver` can function after restoring `persisted` meta.
    let mut receiver = TracingEventReceiver::new(metadata, spans, local_spans);
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        for event in events {
            if !matches!(event, TracingEvent::NewCallSite { .. }) {
                receiver.receive(event.clone());
            }
        }
    });
}

fn test_persisting_spans(reset_local_spans: bool) {
    let events = &EVENTS.short;
    let split_positions = events.iter().enumerate().filter_map(|(i, event)| {
        if matches!(
            event,
            TracingEvent::NewSpan { .. } | TracingEvent::SpanExited { .. }
        ) {
            Some(i + 1)
        } else {
            None
        }
    });
    let split_positions: Vec<_> = iter::once(0)
        .chain(split_positions)
        .chain([events.len()])
        .collect();
    let event_chunks = split_positions.windows(2).map(|window| match window {
        [prev, next] => &events[*prev..*next],
        _ => unreachable!(),
    });

    let mut metadata = PersistedMetadata::default();
    let mut spans = PersistedSpans::default();
    let mut local_spans = LocalSpans::default();
    tracing::subscriber::with_default(create_fmt_subscriber(), || {
        for events in event_chunks {
            if reset_local_spans {
                local_spans = LocalSpans::default();
            }

            let mut receiver = TracingEventReceiver::new(metadata.clone(), spans, local_spans);
            for event in events {
                receiver.receive(event.clone());
            }
            metadata = receiver.persist_metadata();
            (spans, local_spans) = receiver.persist();
        }
    });
}

#[test]
fn persisting_spans() {
    test_persisting_spans(false);
}

#[test]
fn persisting_spans_with_reset_local_spans() {
    test_persisting_spans(true);
}

#[test]
#[allow(clippy::needless_collect)] // necessary for threads to be concurrent
fn concurrent_senders() {
    Lazy::force(&EVENTS);

    let threads: Vec<_> = (5..10)
        .map(|i| thread::spawn(move || fib::record_events(i)))
        .collect();
    let events_by_thread = threads.into_iter().map(|handle| handle.join().unwrap());

    for (idx, events) in events_by_thread.enumerate() {
        assert_valid_refs(&events);
        assert_span_management(&events);

        let idx = idx + 5;
        let new_events: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let TracingEvent::NewEvent { values, .. } = event {
                    return values.get("message").and_then(TracedValue::as_debug_str);
                }
                None
            })
            .collect();

        assert_eq!(new_events.len(), idx + 2, "{new_events:?}");
        assert_eq!(new_events[0], "count looks somewhat large");
        assert_eq!(new_events[idx + 1], "computed Fibonacci number");
        for &new_event in &new_events[1..=idx] {
            assert_eq!(new_event, "performing iteration");
        }
    }
}

#[test]
#[allow(clippy::needless_collect)] // necessary for threads to be concurrent
fn concurrent_senders_stress_test() {
    Lazy::force(&EVENTS);

    // Stress test with more threads and rapid span creation to increase
    // likelihood of race condition between NewCallSite and NewSpan events
    const NUM_THREADS: usize = 20;
    const ITERATIONS: usize = 10;

    let threads: Vec<_> = (0..NUM_THREADS)
        .map(|thread_id| {
            thread::spawn(move || {
                // Use single sender per thread to avoid span ID conflicts
                let (events_sx, events_rx) = std::sync::mpsc::sync_channel(512);
                let sender = TracingEventSender::new(move |event| {
                    events_sx.send(event).unwrap();
                });

                tracing::subscriber::with_default(sender, || {
                    // Mix of different span types to trigger more callsite registrations
                    for i in 0..ITERATIONS {
                        let _span = tracing::info_span!("stress_test", thread_id, iteration = i);
                        tracing::debug!("stress test event");

                        // Nested spans with different metadata
                        let _nested = tracing::warn_span!("nested", value = i * 2);
                        tracing::info!("nested event");
                    }

                    // Additional rapid span creation
                    for i in 0..ITERATIONS {
                        let _rapid = tracing::trace_span!("rapid", count = i);
                        tracing::error!("rapid event {}", i);
                    }
                });

                events_rx.iter().collect()
            })
        })
        .collect();

    let all_events: Vec<Vec<TracingEvent>> = threads
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();

    // Validate each thread's events individually
    for (thread_idx, events) in all_events.iter().enumerate() {
        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_valid_refs(events);
        })) {
            eprintln!("Thread {} events failed validation", thread_idx);
            std::panic::resume_unwind(e);
        }
        assert_span_management(events);
    }
}

fn assert_valid_refs(events: &[TracingEvent]) {
    let mut call_site_ids = HashSet::new();
    let mut span_ids = HashSet::new();
    for (i, event) in events.iter().enumerate() {
        match event {
            TracingEvent::NewCallSite { id, .. } => {
                call_site_ids.insert(*id);
                // IDs may duplicate provided they reference the same call site.
            }
            TracingEvent::NewSpan {
                id, metadata_id, ..
            } => {
                assert!(span_ids.insert(*id));
                if !call_site_ids.contains(metadata_id) {
                    // Enhanced error reporting for potential race condition detection
                    eprintln!("Event ordering issue detected at event {}: NewSpan references unknown metadata_id {}", i, metadata_id);
                    eprintln!("Known call site IDs: {:?}", call_site_ids);
                    eprintln!(
                        "Event context (events {}..{}):",
                        i.saturating_sub(3),
                        (i + 3).min(events.len())
                    );
                    for (j, ctx_event) in events[i.saturating_sub(3)..(i + 3).min(events.len())]
                        .iter()
                        .enumerate()
                    {
                        let event_idx = i.saturating_sub(3) + j;
                        let marker = if event_idx == i { " >>> " } else { "     " };
                        eprintln!("{}[{}] {:?}", marker, event_idx, ctx_event);
                    }
                    panic!("NewSpan event references unknown metadata ID {}: this may be indicative of a race condition where NewSpan arrived before NewCallSite", metadata_id);
                }
            }
            TracingEvent::NewEvent { metadata_id, .. } => {
                if !call_site_ids.contains(metadata_id) {
                    eprintln!("Event ordering issue detected at event {}: NewEvent references unknown metadata_id {}", i, metadata_id);
                    panic!("NewEvent references unknown metadata ID {}: this may be indicative of a race condition", metadata_id);
                }
            }
            _ => { /* do nothing */ }
        }
    }
}
