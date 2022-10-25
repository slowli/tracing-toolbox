//! Tests for tracing event consumer.

use assert_matches::assert_matches;
use tracing_subscriber::{layer::SubscriberExt, Registry};

use std::borrow::Cow;

use super::*;
use crate::{
    capture::{CaptureLayer, SharedStorage},
    CallSiteKind, TracingLevel,
};

const CALL_SITE_DATA: CallSiteData = create_call_site(Vec::new());

const fn create_call_site(fields: Vec<Cow<'static, str>>) -> CallSiteData {
    CallSiteData {
        kind: CallSiteKind::Span,
        name: Cow::Borrowed("test"),
        target: Cow::Borrowed("tardigrade_tracing"),
        level: TracingLevel::Error,
        module_path: Some(Cow::Borrowed("subscriber::tests")),
        file: Some(Cow::Borrowed("tests")),
        line: Some(42),
        fields,
    }
}

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
        let mut consumer = EventConsumer::default();
        for event in events {
            consumer.consume_event(event);
        }
    });

    let storage = storage.lock();
    let span = storage.spans().next().unwrap();
    assert_eq!(span.stats().entered, 2);
    assert_eq!(span.stats().exited, 2);
    assert!(span.stats().is_closed);
}

#[test]
fn unknown_metadata_error() {
    let event = TracingEvent::NewSpan {
        id: 0,
        parent_id: None,
        metadata_id: 0,
        values: vec![],
    };
    let mut consumer = EventConsumer::default();
    let err = consumer.try_consume_event(event).unwrap_err();
    assert_matches!(err, ConsumeError::UnknownMetadataId(0));
}

#[test]
fn unknown_span_errors() {
    let bogus_events = [
        TracingEvent::SpanEntered { id: 1 },
        TracingEvent::SpanExited { id: 1 },
        TracingEvent::SpanDropped { id: 1 },
        TracingEvent::NewSpan {
            id: 42,
            parent_id: Some(1),
            metadata_id: 0,
            values: vec![],
        },
        TracingEvent::NewEvent {
            metadata_id: 0,
            parent: Some(1),
            values: vec![],
        },
        TracingEvent::ValuesRecorded {
            id: 1,
            values: vec![],
        },
    ];

    let mut consumer = EventConsumer::default();
    consumer.consume_event(TracingEvent::NewCallSite {
        id: 0,
        data: CALL_SITE_DATA,
    });
    for bogus_event in bogus_events {
        let err = consumer.try_consume_event(bogus_event).unwrap_err();
        assert_matches!(err, ConsumeError::UnknownSpanId(1));
    }
}

#[test]
fn spans_with_allowed_value_lengths() {
    for values_len in 0..=32 {
        println!("values length: {values_len}");

        let mut consumer = EventConsumer::default();
        let fields = (0..values_len)
            .map(|i| Cow::Owned(format!("field{i}")))
            .collect();
        consumer.consume_event(TracingEvent::NewCallSite {
            id: 0,
            data: create_call_site(fields),
        });

        let values = (0..values_len)
            .map(|i| (format!("field{i}"), TracedValue::Int(i.into())))
            .collect();
        consumer.consume_event(TracingEvent::NewSpan {
            id: 0,
            parent_id: None,
            metadata_id: 0,
            values,
        });
        consumer.consume_event(TracingEvent::SpanDropped { id: 0 });
    }
}

#[test]
fn too_many_values_error() {
    let mut consumer = EventConsumer::default();
    consumer.consume_event(TracingEvent::NewCallSite {
        id: 0,
        data: CALL_SITE_DATA,
    });

    let values = (0..33)
        .map(|i| (format!("field{i}"), TracedValue::Int(i.into())))
        .collect();
    let bogus_event = TracingEvent::NewSpan {
        id: 0,
        parent_id: None,
        metadata_id: 0,
        values,
    };
    let err = consumer.try_consume_event(bogus_event).unwrap_err();
    assert_matches!(
        err,
        ConsumeError::TooManyValues {
            actual: 33,
            max: 32
        }
    );
}
