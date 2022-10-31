//! Capturing tracing spans and events, e.g. for testing purposes.
//!
//! The core type in this crate is [`CaptureLayer`], a tracing [`Layer`] that can be used
//! to capture tracing spans and events.
//!
//! # Examples
//!
//! ```
//! use tracing::Level;
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_capture::{CaptureLayer, SharedStorage};
//!
//! let subscriber = tracing_subscriber::fmt()
//!     .pretty()
//!     .with_max_level(Level::INFO)
//!     .finish();
//! // Add the capturing layer.
//! let storage = SharedStorage::default();
//! let subscriber = subscriber.with(CaptureLayer::new(&storage));
//!
//! // Capture tracing information.
//! tracing::subscriber::with_default(subscriber, || {
//!     tracing::info_span!("test", num = 42_i64).in_scope(|| {
//!         tracing::warn!("I feel disturbance in the Force...");
//!     });
//! });
//!
//! // Inspect the only captured span.
//! let storage = storage.lock();
//! assert_eq!(storage.all_spans().len(), 1);
//! let span = storage.all_spans().next().unwrap();
//! assert_eq!(span["num"], 42_i64);
//! assert_eq!(span.stats().entered, 1);
//! assert!(span.stats().is_closed);
//!
//! // Inspect the only event in the span.
//! let event = span.events().next().unwrap();
//! assert_eq!(*event.metadata().level(), Level::WARN);
//! assert_eq!(
//!     event["message"].as_debug_str(),
//!     Some("I feel disturbance in the Force...")
//! );
//! ```
//!
//! # Alternatives / similar tools
//!
//! - [`tracing-test`] is a lower-level alternative.
//! - [`tracing-fluent-assertions`] is more similar in its goals, but differs significantly
//!   in the API design; e.g., the assertions need to be declared before the capture.
//!
//! [`tracing-test`]: https://docs.rs/tracing-test
//! [`tracing-fluent-assertions`]: https://docs.rs/tracing-fluent-assertions

// Documentation settings.
#![doc(html_root_url = "https://docs.rs/tracing-capture/0.1.0")]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

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

mod iter;
pub mod predicates;

mod sealed {
    pub trait Sealed {}
}

pub use crate::iter::{CapturedEvents, CapturedSpans};

use tracing_tunnel::{TracedValue, TracedValues, ValueVisitor};

/// Marker trait for captured objects (spans and events).
pub trait Captured: fmt::Debug + sealed::Sealed {}

#[derive(Debug)]
struct CapturedEventInner {
    metadata: &'static Metadata<'static>,
    values: TracedValues<&'static str>,
    parent_id: Option<CapturedSpanId>,
}

type CapturedEventId = id_arena::Id<CapturedEventInner>;

/// Captured tracing event containing a reference to its [`Metadata`] and values that the event
/// was created with.
#[derive(Debug, Clone, Copy)]
pub struct CapturedEvent<'a> {
    inner: &'a CapturedEventInner,
    storage: &'a Storage,
}

impl<'a> CapturedEvent<'a> {
    /// Provides a reference to the event metadata.
    pub fn metadata(&self) -> &'static Metadata<'static> {
        self.inner.metadata
    }

    /// Iterates over values associated with the event.
    pub fn values(&self) -> impl Iterator<Item = (&'static str, &TracedValue)> + '_ {
        self.inner.values.iter().map(|(name, value)| (*name, value))
    }

    /// Returns a value for the specified field, or `None` if the value is not defined.
    pub fn value(&self, name: &str) -> Option<&TracedValue> {
        self.inner
            .values
            .iter()
            .find_map(|(s, value)| if *s == name { Some(value) } else { None })
    }

    /// Returns the parent span for this event, or `None` if is not tied to a captured span.
    pub fn parent(&self) -> Option<CapturedSpan<'a>> {
        self.inner.parent_id.map(|id| self.storage.span(id))
    }

    /// Returns the references to the ancestor spans, starting from the direct parent
    /// and ending in one of [root spans](Storage::root_spans()).
    pub fn ancestors(&self) -> impl Iterator<Item = CapturedSpan<'a>> + '_ {
        std::iter::successors(self.parent(), CapturedSpan::parent)
    }
}

impl ops::Index<&str> for CapturedEvent<'_> {
    type Output = TracedValue;

    fn index(&self, index: &str) -> &Self::Output {
        self.value(index)
            .unwrap_or_else(|| panic!("field `{index}` is not contained in event"))
    }
}

impl sealed::Sealed for CapturedEvent<'_> {}
impl Captured for CapturedEvent<'_> {}

/// Statistics about a [`CapturedSpan`].
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct SpanStats {
    /// Number of times the span was entered.
    pub entered: usize,
    /// Number of times the span was exited.
    pub exited: usize,
    /// Is the span closed (dropped)?
    pub is_closed: bool,
}

#[derive(Debug)]
struct CapturedSpanInner {
    metadata: &'static Metadata<'static>,
    values: TracedValues<&'static str>,
    stats: SpanStats,
    parent_id: Option<CapturedSpanId>,
    child_ids: Vec<CapturedSpanId>,
    event_ids: Vec<CapturedEventId>,
}

type CapturedSpanId = id_arena::Id<CapturedSpanInner>;

/// Captured tracing span containing a reference to its [`Metadata`], values that the span
/// was created with, [stats](SpanStats), and descendant [`CapturedEvent`]s.
#[derive(Debug, Clone, Copy)]
pub struct CapturedSpan<'a> {
    inner: &'a CapturedSpanInner,
    storage: &'a Storage,
}

impl<'a> CapturedSpan<'a> {
    /// Provides a reference to the span metadata.
    pub fn metadata(&self) -> &'static Metadata<'static> {
        self.inner.metadata
    }

    /// Iterates over values that the span was created with, or which were recorded later.
    pub fn values(&self) -> impl Iterator<Item = (&'static str, &TracedValue)> + '_ {
        self.inner.values.iter().map(|(name, value)| (*name, value))
    }

    /// Returns a value for the specified field, or `None` if the value is not defined.
    pub fn value(&self, name: &str) -> Option<&TracedValue> {
        self.inner
            .values
            .iter()
            .find_map(|(s, value)| if *s == name { Some(value) } else { None })
    }

    /// Returns statistics about span operations.
    pub fn stats(&self) -> SpanStats {
        self.inner.stats
    }

    /// Returns events attached to this span.
    pub fn events(&self) -> CapturedEvents<'a> {
        CapturedEvents::from_slice(self.storage, &self.inner.event_ids)
    }

    /// Returns the reference to the parent span, if any.
    pub fn parent(&self) -> Option<Self> {
        self.inner.parent_id.map(|id| self.storage.span(id))
    }

    /// Returns the references to the ancestor spans, starting from the direct parent
    /// and ending in one of [root spans](Storage::root_spans()).
    pub fn ancestors(&self) -> impl Iterator<Item = CapturedSpan<'a>> + '_ {
        std::iter::successors(self.parent(), Self::parent)
    }

    /// Iterates over direct children of this span, in the order of their capture.
    pub fn children(&self) -> CapturedSpans<'a> {
        CapturedSpans::from_slice(self.storage, &self.inner.child_ids)
    }
}

impl ops::Index<&str> for CapturedSpan<'_> {
    type Output = TracedValue;

    fn index(&self, index: &str) -> &Self::Output {
        self.value(index)
            .unwrap_or_else(|| panic!("field `{index}` is not contained in span"))
    }
}

impl sealed::Sealed for CapturedSpan<'_> {}
impl Captured for CapturedSpan<'_> {}

/// Storage of captured tracing information.
///
/// `Storage` instances are not created directly; instead, they are wrapped in [`SharedStorage`]
/// and can be accessed via [`lock()`](SharedStorage::lock()).
#[derive(Debug)]
pub struct Storage {
    spans: Arena<CapturedSpanInner>,
    root_span_ids: Vec<CapturedSpanId>,
    events: Arena<CapturedEventInner>,
    root_event_ids: Vec<CapturedEventId>,
}

impl Storage {
    fn new() -> Self {
        Self {
            spans: Arena::new(),
            root_span_ids: vec![],
            events: Arena::new(),
            root_event_ids: vec![],
        }
    }

    fn span(&self, id: CapturedSpanId) -> CapturedSpan<'_> {
        CapturedSpan {
            inner: &self.spans[id],
            storage: self,
        }
    }

    fn event(&self, id: CapturedEventId) -> CapturedEvent<'_> {
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

    fn push_span(
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

    fn push_event(
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

#[cfg(doctest)]
doc_comment::doctest!("../README.md");
