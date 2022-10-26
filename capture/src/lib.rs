//! Capturing tracing spans, e.g. for testing purposes.
//!
//! The core type in this crate is [`CaptureLayer`], a tracing [`Layer`] that can be used
//! to capture tracing spans. See its docs for more details.

// Documentation settings.
#![doc(html_root_url = "https://docs.rs/tracing-capture/0.1.0")]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use tracing_core::{
    span::{Attributes, Id, Record},
    LevelFilter, Metadata, Subscriber,
};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

use std::{
    ops,
    sync::{Arc, Mutex},
};

use tracing_tunnel::{TracedValue, ValueVisitor};

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
/// was created with, and [stats](SpanStats).
#[derive(Debug)]
pub struct CapturedSpan {
    metadata: &'static Metadata<'static>,
    values: Vec<(&'static str, TracedValue)>, // FIXME: use LinkedHashMap
    stats: SpanStats,
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
}

impl Storage {
    fn new() -> Self {
        Self { spans: vec![] }
    }

    /// Iterates over all captured spans in the order of capture.
    pub fn spans(&self) -> impl Iterator<Item = &CapturedSpan> + '_ {
        self.spans.iter()
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

    fn on_record(&mut self, idx: usize, values: Vec<(&'static str, TracedValue)>) {
        let span = self.spans.get_mut(idx).unwrap();
        span.values.extend(values);
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

impl SharedStorage {
    /// Locks the underlying [`Storage`] for exclusive access. While the lock is held,
    /// capturing cannot progress; beware deadlocks!
    #[allow(clippy::missing_panics_doc)]
    pub fn lock(&self) -> impl ops::Deref<Target = Storage> + '_ {
        self.inner.lock().unwrap()
    }
}

#[derive(Debug, Clone, Copy)]
struct SpanIndex(usize);

#[derive(Debug)]
struct SimpleFilter {
    target: String,
    level: LevelFilter,
}

impl SimpleFilter {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == self.target && *metadata.level() <= self.level
    }
}

/// Tracing [`Layer`] that captures (optionally filtered) spans.
#[derive(Debug)]
pub struct CaptureLayer {
    filter: Option<SimpleFilter>,
    storage: Arc<Mutex<Storage>>,
}

impl CaptureLayer {
    /// Creates a new layer that will use the specified `storage` to store captured data.
    /// Captured spans are not filtered; like any [`Layer`], filtering can be set up
    /// on the layer or subscriber level.
    pub fn new(storage: &SharedStorage) -> Self {
        Self {
            filter: None,
            storage: Arc::clone(&storage.inner),
        }
    }

    /// Specifies filtering for this layer. This can be used for cheap per-layer filtering if
    /// it is not supported by the tracing subscriber.
    #[must_use]
    pub fn with_filter(mut self, target: impl Into<String>, level: impl Into<LevelFilter>) -> Self {
        self.filter = Some(SimpleFilter {
            target: target.into(),
            level: level.into(),
        });
        self
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.filter
            .as_ref()
            .map_or(true, |filter| filter.enabled(metadata))
    }

    fn lock(&self) -> impl ops::DerefMut<Target = Storage> + '_ {
        self.storage.lock().unwrap()
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if !self.enabled(attrs.metadata()) {
            return;
        }

        let mut visitor = ValueVisitor::default();
        attrs.record(&mut visitor);
        let span = CapturedSpan {
            metadata: attrs.metadata(),
            values: visitor.values,
            stats: SpanStats::default(),
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
