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
//! assert_eq!(storage.spans().len(), 1);
//! let span = &storage.spans()[0];
//! assert_eq!(span["num"], 42_i64);
//! assert_eq!(span.stats().entered, 1);
//! assert!(span.stats().is_closed);
//!
//! // Inspect the only event in the span.
//! let event = &span.events()[0];
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

pub mod predicates;

use tracing_tunnel::{TracedValue, TracedValues, ValueVisitor};

/// Captured tracing event containing a reference to its [`Metadata`] and values that the event
/// was created with.
#[derive(Debug)]
pub struct CapturedEvent {
    metadata: &'static Metadata<'static>,
    values: TracedValues<&'static str>,
}

impl CapturedEvent {
    /// Provides a reference to the event metadata.
    pub fn metadata(&self) -> &'static Metadata<'static> {
        self.metadata
    }

    /// Iterates over values associated with the event.
    pub fn values(&self) -> impl Iterator<Item = (&'static str, &TracedValue)> + '_ {
        self.values.iter().map(|(name, value)| (*name, value))
    }

    /// Returns a value for the specified field, or `None` if the value is not defined.
    pub fn value(&self, name: &str) -> Option<&TracedValue> {
        self.values
            .iter()
            .find_map(|(s, value)| if *s == name { Some(value) } else { None })
    }
}

impl ops::Index<&str> for CapturedEvent {
    type Output = TracedValue;

    fn index(&self, index: &str) -> &Self::Output {
        self.value(index)
            .unwrap_or_else(|| panic!("field `{index}` is not contained in event"))
    }
}

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

/// Captured tracing span containing a reference to its [`Metadata`], values that the span
/// was created with, [stats](SpanStats), and descendant [`CapturedEvent`]s.
#[derive(Debug)]
pub struct CapturedSpan {
    metadata: &'static Metadata<'static>,
    values: TracedValues<&'static str>,
    stats: SpanStats,
    events: Vec<CapturedEvent>,
}

impl CapturedSpan {
    /// Provides a reference to the span metadata.
    pub fn metadata(&self) -> &'static Metadata<'static> {
        self.metadata
    }

    /// Iterates over values that the span was created with, or which were recorded later.
    pub fn values(&self) -> impl Iterator<Item = (&'static str, &TracedValue)> + '_ {
        self.values.iter().map(|(name, value)| (*name, value))
    }

    /// Returns a value for the specified field, or `None` if the value is not defined.
    pub fn value(&self, name: &str) -> Option<&TracedValue> {
        self.values
            .iter()
            .find_map(|(s, value)| if *s == name { Some(value) } else { None })
    }

    /// Returns statistics about span operations.
    pub fn stats(&self) -> SpanStats {
        self.stats
    }

    /// Returns events attached to this span.
    pub fn events(&self) -> &[CapturedEvent] {
        &self.events
    }
}

impl ops::Index<&str> for CapturedSpan {
    type Output = TracedValue;

    fn index(&self, index: &str) -> &Self::Output {
        self.value(index)
            .unwrap_or_else(|| panic!("field `{index}` is not contained in span"))
    }
}

/// Storage of captured tracing information.
///
/// `Storage` instances are not created directly; instead, they are wrapped in [`SharedStorage`]
/// and can be accessed via [`lock()`](SharedStorage::lock()).
#[derive(Debug)]
pub struct Storage {
    spans: Vec<CapturedSpan>,
    root_events: Vec<CapturedEvent>,
}

impl Storage {
    fn new() -> Self {
        Self {
            spans: vec![],
            root_events: vec![],
        }
    }

    /// Returns captured spans in the order of capture.
    pub fn spans(&self) -> &[CapturedSpan] {
        &self.spans
    }

    /// Returns captured root events (i.e., events that were emitted when no captured span
    /// was entered) in the order of capture.
    pub fn root_events(&self) -> &[CapturedEvent] {
        &self.root_events
    }

    /// Iterates over all captured events. The order of iteration is not specified.
    pub fn all_events(&self) -> impl Iterator<Item = &CapturedEvent> + '_ {
        self.spans
            .iter()
            .flat_map(CapturedSpan::events)
            .chain(&self.root_events)
    }

    fn push_span(&mut self, span: CapturedSpan) -> usize {
        let idx = self.spans.len();
        self.spans.push(span);
        idx
    }

    fn on_span_enter(&mut self, idx: usize) {
        let span = self.spans.get_mut(idx).unwrap();
        span.stats.entered += 1;
    }

    fn on_span_exit(&mut self, idx: usize) {
        let span = self.spans.get_mut(idx).unwrap();
        span.stats.exited += 1;
    }

    fn on_span_closed(&mut self, idx: usize) {
        let span = self.spans.get_mut(idx).unwrap();
        span.stats.is_closed = true;
    }

    fn on_record(&mut self, idx: usize, values: TracedValues<&'static str>) {
        let span = self.spans.get_mut(idx).unwrap();
        span.values.extend(values);
    }

    fn on_event(&mut self, span_idx: Option<usize>, event: CapturedEvent) {
        if let Some(span_idx) = span_idx {
            let span = self.spans.get_mut(span_idx).unwrap();
            span.events.push(event);
        } else {
            self.root_events.push(event);
        }
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

#[derive(Debug, Clone, Copy)]
struct SpanIndex(usize);

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

        let mut visitor = ValueVisitor::default();
        attrs.record(&mut visitor);
        let span = CapturedSpan {
            metadata: attrs.metadata(),
            values: visitor.values,
            stats: SpanStats::default(),
            events: vec![],
        };
        let idx = self.lock().push_span(span);
        ctx.span(id)
            .unwrap()
            .extensions_mut()
            .insert(SpanIndex(idx));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        if let Some(SpanIndex(idx)) = span.extensions().get::<SpanIndex>().copied() {
            let mut visitor = ValueVisitor::default();
            values.record(&mut visitor);
            self.lock().on_record(idx, visitor.values);
        };
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !self.enabled(event.metadata(), &ctx) {
            return;
        }

        let ancestor_span = if let Some(mut scope) = ctx.event_scope(event) {
            scope.find_map(|span| span.extensions().get::<SpanIndex>().copied())
        } else {
            None
        };
        let span_idx = ancestor_span.map(|idx| idx.0);
        let mut visitor = ValueVisitor::default();
        event.record(&mut visitor);
        let event = CapturedEvent {
            metadata: event.metadata(),
            values: visitor.values,
        };
        self.lock().on_event(span_idx, event);
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        if let Some(SpanIndex(idx)) = span.extensions().get::<SpanIndex>().copied() {
            self.lock().on_span_enter(idx);
        };
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        if let Some(SpanIndex(idx)) = span.extensions().get::<SpanIndex>().copied() {
            self.lock().on_span_exit(idx);
        };
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let span = ctx.span(&id).unwrap();
        if let Some(SpanIndex(idx)) = span.extensions().get::<SpanIndex>().copied() {
            self.lock().on_span_closed(idx);
        };
    }
}

#[cfg(doctest)]
doc_comment::doctest!("../README.md");
