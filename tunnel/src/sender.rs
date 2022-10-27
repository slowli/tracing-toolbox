//! Client-side subscriber.

use tracing_core::{
    callsite::Identifier,
    span::{Attributes, Id, Record},
    Event, Interest, Metadata, Subscriber,
};

use std::{
    collections::HashMap,
    ops,
    sync::{
        atomic::{AtomicU64, Ordering},
        RwLock,
    },
};

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

#[derive(Debug, Default)]
struct Inner {
    call_sites: HashMap<Identifier, MetadataId>,
    next_metadata_id: MetadataId,
}

impl Inner {
    /// Returns metadata ID together with a flag whether it is new.
    fn register_site(&mut self, metadata: &'static Metadata<'static>) -> (MetadataId, bool) {
        let site_id = metadata.callsite();
        if let Some(&metadata_id) = self.call_sites.get(&site_id) {
            return (metadata_id, false);
        }

        let metadata_id = self.next_metadata_id;
        self.next_metadata_id += 1;
        self.call_sites.insert(site_id, metadata_id);
        (metadata_id, true)
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
    inner: RwLock<Inner>,
    next_span_id: AtomicU64,
    on_event: F,
}

impl<F: Fn(TracingEvent) + 'static> TracingEventSender<F> {
    /// Creates a subscriber with the specified "on event" hook.
    pub fn new(on_event: F) -> Self {
        Self {
            inner: RwLock::default(),
            next_span_id: AtomicU64::new(1), // 0 is invalid span ID
            on_event,
        }
    }

    fn lock_read(&self) -> impl ops::Deref<Target = Inner> + '_ {
        self.inner.read().unwrap()
    }

    fn lock_write(&self) -> impl ops::DerefMut<Target = Inner> + '_ {
        self.inner.write().unwrap()
    }

    fn metadata_id(&self, metadata: &'static Metadata<'static>) -> MetadataId {
        let maybe_metadata_id = self
            .lock_read()
            .call_sites
            .get(&metadata.callsite())
            .copied();
        maybe_metadata_id.unwrap_or_else(|| {
            let mut lock = self.lock_write();
            let (id, is_new) = lock.register_site(metadata);
            if is_new {
                // **NB.** It is imperative that the write lock is held here!
                // Otherwise, event emission may be reordered (`NewCallSite` after
                // a `NewSpan` / `NewEvent` referencing it).
                self.send(TracingEvent::NewCallSite {
                    id,
                    data: CallSiteData::from(metadata),
                });
            }
            id
        })
    }

    fn send(&self, event: TracingEvent) {
        (self.on_event)(event);
    }
}

impl<F: Fn(TracingEvent) + 'static> Subscriber for TracingEventSender<F> {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        self.metadata_id(metadata); // registers metadata if necessary
        Interest::always()
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let metadata_id = self.metadata_id(span.metadata());
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
        let metadata_id = self.metadata_id(event.metadata());
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
