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

use crate::types::{MetadataId, TracingEvent};

#[derive(Debug, Default)]
struct Inner {
    call_sites: HashMap<Identifier, MetadataId>,
    next_metadata_id: MetadataId,
}

impl Inner {
    fn register_site(&mut self, metadata: &'static Metadata<'static>) -> MetadataId {
        let site_id = metadata.callsite();
        debug_assert!(self.call_sites.get(&site_id).is_none());
        let metadata_id = self.next_metadata_id;
        self.next_metadata_id += 1;
        self.call_sites.insert(site_id, metadata_id);
        metadata_id
    }
}

#[derive(Debug)]
pub struct EmittingSubscriber<F = fn(TracingEvent)> {
    inner: RwLock<Inner>,
    next_span_id: AtomicU64,
    on_event: F,
}

impl<F: Fn(TracingEvent) + 'static> EmittingSubscriber<F> {
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

    fn emit(&self, event: TracingEvent) {
        (self.on_event)(event);
    }
}

impl<F: Fn(TracingEvent) + 'static> Subscriber for EmittingSubscriber<F> {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        let metadata_id = self.lock_write().register_site(metadata);
        self.emit(TracingEvent::new_call_site(metadata, metadata_id));
        Interest::always()
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true // FIXME: reasonable implementation
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let metadata_id = self.lock_read().call_sites[&span.metadata().callsite()];
        let span_id = self.next_span_id.fetch_add(1, Ordering::SeqCst);
        self.emit(TracingEvent::new_span(span, metadata_id, span_id));
        Id::from_u64(span_id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        self.emit(TracingEvent::values_recorded(span.into_u64(), values));
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        self.emit(TracingEvent::FollowsFrom {
            id: span.into_u64(),
            follows_from: follows.into_u64(),
        });
    }

    fn event(&self, event: &Event<'_>) {
        let metadata_id = self.lock_read().call_sites[&event.metadata().callsite()];
        self.emit(TracingEvent::new_event(event, metadata_id));
    }

    fn enter(&self, span: &Id) {
        self.emit(TracingEvent::SpanEntered {
            id: span.into_u64(),
        });
    }

    fn exit(&self, span: &Id) {
        self.emit(TracingEvent::SpanExited {
            id: span.into_u64(),
        });
    }

    fn clone_span(&self, span: &Id) -> Id {
        self.emit(TracingEvent::SpanCloned {
            id: span.into_u64(),
        });
        span.clone()
    }

    fn try_close(&self, span: Id) -> bool {
        self.emit(TracingEvent::SpanDropped {
            id: span.into_u64(),
        });
        false
    }
}
