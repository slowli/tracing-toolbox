//! Client-side subscriber.

use tracing_core::{
    span::{Attributes, Id, Record},
    Event, Interest, Metadata, Subscriber,
};

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{types::ValueVisitor, CallSiteData, MetadataId, RawSpanId, TracingEvent};

impl TracingEvent {
    fn new_span(span: &Attributes<'_>, metadata_id: MetadataId, id: RawSpanId) -> Self {
        let mut visitor = ValueVisitor::default();
        span.record(&mut visitor);
        Self::NewSpan {
            id,
            parent_id: span.parent().map(Id::into_u64),
            metadata_id,
            values: visitor.values,
        }
    }

    fn values_recorded(id: RawSpanId, values: &Record<'_>) -> Self {
        let mut visitor = ValueVisitor::default();
        values.record(&mut visitor);
        Self::ValuesRecorded {
            id,
            values: visitor.values,
        }
    }

    fn new_event(event: &Event<'_>, metadata_id: MetadataId) -> Self {
        let mut visitor = ValueVisitor::default();
        event.record(&mut visitor);
        Self::NewEvent {
            metadata_id,
            parent: event.parent().map(Id::into_u64),
            values: visitor.values,
        }
    }
}

/// Tracing [`Subscriber`] that converts tracing events into (de)serializable [presentation]
/// that can be sent elsewhere using a customizable hook.
///
/// As an example, this subscriber is used in the [Tardigrade client library] to send
/// workflow traces to the host via a WASM import function.
///
/// # Examples
///
/// See [crate-level docs](index.html) for an example of usage.
///
/// [presentation]: TracingEvent
/// [Tardigrade client library]: https://docs.rs/tardigrade
#[derive(Debug)]
pub struct TracingEventSender<F = fn(TracingEvent)> {
    next_span_id: AtomicU64,
    on_event: F,
}

impl<F: Fn(TracingEvent) + 'static> TracingEventSender<F> {
    /// Creates a subscriber with the specified "on event" hook.
    pub fn new(on_event: F) -> Self {
        Self {
            next_span_id: AtomicU64::new(1), // 0 is invalid span ID
            on_event,
        }
    }

    fn metadata_id(metadata: &'static Metadata<'static>) -> MetadataId {
        metadata as *const _ as MetadataId
    }

    fn send(&self, event: TracingEvent) {
        (self.on_event)(event);
    }
}

impl<F: Fn(TracingEvent) + 'static> Subscriber for TracingEventSender<F> {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        let id = Self::metadata_id(metadata);
        self.send(TracingEvent::NewCallSite {
            id,
            data: CallSiteData::from(metadata),
        });
        Interest::always()
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let metadata_id = Self::metadata_id(span.metadata());
        let span_id = self.next_span_id.fetch_add(1, Ordering::SeqCst);
        self.send(TracingEvent::new_span(span, metadata_id, span_id));
        Id::from_u64(span_id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        self.send(TracingEvent::values_recorded(span.into_u64(), values));
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        self.send(TracingEvent::FollowsFrom {
            id: span.into_u64(),
            follows_from: follows.into_u64(),
        });
    }

    fn event(&self, event: &Event<'_>) {
        let metadata_id = Self::metadata_id(event.metadata());
        self.send(TracingEvent::new_event(event, metadata_id));
    }

    fn enter(&self, span: &Id) {
        self.send(TracingEvent::SpanEntered {
            id: span.into_u64(),
        });
    }

    fn exit(&self, span: &Id) {
        self.send(TracingEvent::SpanExited {
            id: span.into_u64(),
        });
    }

    fn clone_span(&self, span: &Id) -> Id {
        self.send(TracingEvent::SpanCloned {
            id: span.into_u64(),
        });
        span.clone()
    }

    fn try_close(&self, span: Id) -> bool {
        self.send(TracingEvent::SpanDropped {
            id: span.into_u64(),
        });
        false
    }
}
