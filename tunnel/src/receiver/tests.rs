//! Tests for tracing event receiver.

use assert_matches::assert_matches;

use std::borrow::Cow;

use super::*;
use crate::{CallSiteKind, TracingLevel};

const CALL_SITE_DATA: CallSiteData = create_call_site(Vec::new());

const fn create_call_site(fields: Vec<Cow<'static, str>>) -> CallSiteData {
    CallSiteData {
        kind: CallSiteKind::Span,
        name: Cow::Borrowed("test"),
        target: Cow::Borrowed("tracing_tunnel"),
        level: TracingLevel::Error,
        module_path: Some(Cow::Borrowed("receiver::tests")),
        file: Some(Cow::Borrowed("tests")),
        line: Some(42),
        fields,
    }
}

#[test]
fn unknown_metadata_error() {
    let event = TracingEvent::NewSpan {
        id: 0,
        parent_id: None,
        metadata_id: 0,
        values: TracedValues::new(),
    };
    let mut spans = PersistedSpans::default();
    let mut local_spans = LocalSpans::default();
    let mut receiver =
        TracingEventReceiver::new(PersistedMetadata::default(), &mut spans, &mut local_spans);
    let err = receiver.try_receive(event).unwrap_err();
    assert_matches!(err, ReceiveError::UnknownMetadataId(0));
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
            values: TracedValues::new(),
        },
        TracingEvent::NewEvent {
            metadata_id: 0,
            parent: Some(1),
            values: TracedValues::new(),
        },
        TracingEvent::ValuesRecorded {
            id: 1,
            values: TracedValues::new(),
        },
    ];

    let mut spans = PersistedSpans::default();
    let mut local_spans = LocalSpans::default();
    let mut receiver =
        TracingEventReceiver::new(PersistedMetadata::default(), &mut spans, &mut local_spans);
    receiver.receive(TracingEvent::NewCallSite {
        id: 0,
        data: CALL_SITE_DATA,
    });
    for bogus_event in bogus_events {
        let err = receiver.try_receive(bogus_event).unwrap_err();
        assert_matches!(err, ReceiveError::UnknownSpanId(1));
    }
}

#[test]
fn spans_with_allowed_value_lengths() {
    for values_len in 0..=32 {
        println!("values length: {values_len}");

        let mut spans = PersistedSpans::default();
        let mut local_spans = LocalSpans::default();
        let mut receiver =
            TracingEventReceiver::new(PersistedMetadata::default(), &mut spans, &mut local_spans);
        let fields = (0..values_len)
            .map(|i| Cow::Owned(format!("field{i}")))
            .collect();
        receiver.receive(TracingEvent::NewCallSite {
            id: 0,
            data: create_call_site(fields),
        });

        let values = (0..values_len)
            .map(|i| (format!("field{i}"), TracedValue::Int(i.into())))
            .collect();
        receiver.receive(TracingEvent::NewSpan {
            id: 0,
            parent_id: None,
            metadata_id: 0,
            values,
        });
        receiver.receive(TracingEvent::SpanDropped { id: 0 });
    }
}

#[test]
fn too_many_values_error() {
    let mut spans = PersistedSpans::default();
    let mut local_spans = LocalSpans::default();
    let mut receiver =
        TracingEventReceiver::new(PersistedMetadata::default(), &mut spans, &mut local_spans);
    receiver.receive(TracingEvent::NewCallSite {
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
    let err = receiver.try_receive(bogus_event).unwrap_err();
    assert_matches!(
        err,
        ReceiveError::TooManyValues {
            actual: 33,
            max: 32
        }
    );
}

// FIXME: test restoring spans
