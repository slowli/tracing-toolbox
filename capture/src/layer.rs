//! `CaptureLayer` and related types.

use id_arena::Arena;
use tracing_core::{
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};
use tracing_subscriber::{
    layer::{Context, Filter},
    registry::LookupSpan,
    Layer,
};

use std::{
    fmt, ops,
    sync::{Arc, Mutex},
};

use crate::{
    CapturedEvent, CapturedEventId, CapturedEventInner, CapturedEvents, CapturedSpan,
    CapturedSpanId, CapturedSpanInner, CapturedSpans, SpanStats,
};
use tracing_tunnel::{TracedValues, ValueVisitor};

/// Storage of captured tracing information.
///
/// `Storage` instances are not created directly; instead, they are wrapped in [`SharedStorage`]
/// and can be accessed via [`lock()`](SharedStorage::lock()).
#[derive(Debug)]
pub struct Storage {
    pub(crate) spans: Arena<CapturedSpanInner>,
    pub(crate) events: Arena<CapturedEventInner>,
    root_span_ids: Vec<CapturedSpanId>,
    root_event_ids: Vec<CapturedEventId>,
}

impl Storage {
    pub(crate) fn new() -> Self {
        Self {
            spans: Arena::new(),
            events: Arena::new(),
            root_span_ids: vec![],
            root_event_ids: vec![],
        }
    }

    pub(crate) fn span(&self, id: CapturedSpanId) -> CapturedSpan<'_> {
        CapturedSpan {
            inner: &self.spans[id],
            storage: self,
        }
    }

    pub(crate) fn event(&self, id: CapturedEventId) -> CapturedEvent<'_> {
        CapturedEvent {
            inner: &self.events[id],
            storage: self,
        }
    }

    /// Iterates over captured spans in the order of capture.
    pub fn all_spans(&self) -> CapturedSpans<'_> {
        CapturedSpans::from_arena(self)
    }

    /// Iterates over root spans (i.e., spans that do not have a captured parent span)
    /// in the order of capture.
    pub fn root_spans(&self) -> CapturedSpans<'_> {
        CapturedSpans::from_slice(self, &self.root_span_ids)
    }

    /// Iterates over all captured events in the order of capture.
    pub fn all_events(&self) -> CapturedEvents<'_> {
        CapturedEvents::from_arena(self)
    }

    /// Iterates over root events (i.e., events that do not have a captured parent span)
    /// in the order of capture.
    pub fn root_events(&self) -> CapturedEvents<'_> {
        CapturedEvents::from_slice(self, &self.root_event_ids)
    }

    pub(crate) fn push_span(
        &mut self,
        metadata: &'static Metadata<'static>,
        values: TracedValues<&'static str>,
        parent_id: Option<CapturedSpanId>,
    ) -> CapturedSpanId {
        let span_id = self.spans.alloc(CapturedSpanInner {
            metadata,
            values,
            stats: SpanStats::default(),
            parent_id,
            child_ids: vec![],
            event_ids: vec![],
        });
        if let Some(parent_id) = parent_id {
            let span = self.spans.get_mut(parent_id).unwrap();
            span.child_ids.push(span_id);
        } else {
            self.root_span_ids.push(span_id);
        }
        span_id
    }

    fn on_span_enter(&mut self, id: CapturedSpanId) {
        let span = self.spans.get_mut(id).unwrap();
        span.stats.entered += 1;
    }

    fn on_span_exit(&mut self, id: CapturedSpanId) {
        let span = self.spans.get_mut(id).unwrap();
        span.stats.exited += 1;
    }

    fn on_span_closed(&mut self, id: CapturedSpanId) {
        let span = self.spans.get_mut(id).unwrap();
        span.stats.is_closed = true;
    }

    fn on_record(&mut self, id: CapturedSpanId, values: TracedValues<&'static str>) {
        let span = self.spans.get_mut(id).unwrap();
        span.values.extend(values);
    }

    pub(crate) fn push_event(
        &mut self,
        metadata: &'static Metadata<'static>,
        values: TracedValues<&'static str>,
        parent_id: Option<CapturedSpanId>,
    ) -> CapturedEventId {
        let event_id = self.events.alloc(CapturedEventInner {
            metadata,
            values,
            parent_id,
        });
        if let Some(parent_id) = parent_id {
            let span = self.spans.get_mut(parent_id).unwrap();
            span.event_ids.push(event_id);
        } else {
            self.root_event_ids.push(event_id);
        }
        event_id
    }
}

/// Shared wrapper for tracing [`Storage`].
#[derive(Debug, Clone)]
pub struct SharedStorage {
    inner: Arc<Mutex<Storage>>,
}

impl Default for SharedStorage {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Storage::new())),
        }
    }
}

#[allow(clippy::missing_panics_doc)] // lock poisoning propagation
impl SharedStorage {
    /// Locks the underlying [`Storage`] for exclusive access. While the lock is held,
    /// capturing cannot progress; beware of deadlocks!
    pub fn lock(&self) -> impl ops::Deref<Target = Storage> + '_ {
        self.inner.lock().unwrap()
    }
}

/// Tracing [`Layer`] that captures (optionally filtered) spans and events.
///
/// The layer can optionally filter spans and events in addition to global [`Subscriber`] filtering.
/// This could be used instead of per-layer filtering if it's not supported by the `Subscriber`.
/// Keep in mind that without filtering, `CaptureLayer` can capture a lot of
/// unnecessary spans / events.
///
/// Captured events are [tied](CapturedSpan::events()) to the nearest captured span
/// in the span hierarchy. If no entered spans are captured when the event is emitted,
/// the event will be captured in [`Storage::root_events()`].
///
/// # Examples
///
/// See [crate-level docs](index.html) for an example of usage.
pub struct CaptureLayer<S> {
    filter: Option<Box<dyn Filter<S> + Send + Sync>>,
    storage: Arc<Mutex<Storage>>,
}

impl<S> fmt::Debug for CaptureLayer<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CaptureLayer")
            .field("filter", &self.filter.as_ref().map(|_| "Filter"))
            .field("storage", &self.storage)
            .finish()
    }
}

impl<S> CaptureLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    /// Creates a new layer that will use the specified `storage` to store captured data.
    /// Captured spans are not filtered; like any [`Layer`], filtering can be set up
    /// on the layer or subscriber level.
    pub fn new(storage: &SharedStorage) -> Self {
        Self {
            filter: None,
            storage: Arc::clone(&storage.inner),
        }
    }

    /// Specifies filtering for this layer. Unlike with [per-layer filtering](Layer::with_filter()),
    /// the resulting layer will perform filtering for all [`Subscriber`]s, not just [`Registry`].
    ///
    /// [`Registry`]: tracing_subscriber::Registry
    #[must_use]
    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: Filter<S> + Send + Sync + 'static,
    {
        self.filter = Some(Box::new(filter));
        self
    }

    fn enabled(&self, metadata: &Metadata<'_>, ctx: &Context<'_, S>) -> bool {
        self.filter
            .as_deref()
            .map_or(true, |filter| filter.enabled(metadata, ctx))
    }

    fn lock(&self) -> impl ops::DerefMut<Target = Storage> + '_ {
        self.storage.lock().unwrap()
    }
}

impl<S> Layer<S> for CaptureLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if !self.enabled(attrs.metadata(), &ctx) {
            return;
        }

        let parent_id = if let Some(mut scope) = ctx.span_scope(id) {
            scope.find_map(|span| span.extensions().get::<CapturedSpanId>().copied())
        } else {
            None
        };
        let mut visitor = ValueVisitor::default();
        attrs.record(&mut visitor);
        let arena_id = self
            .lock()
            .push_span(attrs.metadata(), visitor.values, parent_id);
        ctx.span(id).unwrap().extensions_mut().insert(arena_id);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        if let Some(id) = span.extensions().get::<CapturedSpanId>().copied() {
            let mut visitor = ValueVisitor::default();
            values.record(&mut visitor);
            self.lock().on_record(id, visitor.values);
        };
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !self.enabled(event.metadata(), &ctx) {
            return;
        }

        let parent_id = if let Some(mut scope) = ctx.event_scope(event) {
            scope.find_map(|span| span.extensions().get::<CapturedSpanId>().copied())
        } else {
            None
        };
        let mut visitor = ValueVisitor::default();
        event.record(&mut visitor);
        self.lock()
            .push_event(event.metadata(), visitor.values, parent_id);
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        if let Some(id) = span.extensions().get::<CapturedSpanId>().copied() {
            self.lock().on_span_enter(id);
        };
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        if let Some(id) = span.extensions().get::<CapturedSpanId>().copied() {
            self.lock().on_span_exit(id);
        };
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let span = ctx.span(&id).unwrap();
        if let Some(id) = span.extensions().get::<CapturedSpanId>().copied() {
            self.lock().on_span_closed(id);
        };
    }
}
