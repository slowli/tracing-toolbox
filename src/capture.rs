//! Capturing spans.

use tracing_core::{
    span::{Attributes, Id, Record},
    LevelFilter, Metadata, Subscriber,
};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

use std::{
    ops,
    sync::{Arc, Mutex},
};

use crate::{types::ValueVisitor, TracedValue};

#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct SpanStats {
    pub entered: usize,
    pub exited: usize,
    pub is_closed: bool,
}

#[derive(Debug)]
pub struct CapturedSpan {
    metadata: &'static Metadata<'static>,
    values: Vec<(&'static str, TracedValue)>,
    stats: SpanStats,
}

impl CapturedSpan {
    pub fn metadata(&self) -> &'static Metadata<'static> {
        self.metadata
    }

    pub fn values(&self) -> impl Iterator<Item = (&'static str, &TracedValue)> + '_ {
        self.values.iter().map(|(name, value)| (*name, value))
    }

    pub fn value(&self, name: &str) -> Option<&TracedValue> {
        self.values
            .iter()
            .find_map(|(s, value)| if *s == name { Some(value) } else { None })
    }

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

#[derive(Debug, Default)]
pub struct Storage {
    spans: Vec<CapturedSpan>,
}

impl Storage {
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

#[derive(Debug, Clone, Default)]
pub struct SharedStorage {
    inner: Arc<Mutex<Storage>>,
}

impl SharedStorage {
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

#[derive(Debug)]
pub struct CaptureLayer {
    filter: Option<SimpleFilter>,
    storage: Arc<Mutex<Storage>>,
}

impl CaptureLayer {
    pub fn new(storage: &SharedStorage) -> Self {
        Self {
            filter: None,
            storage: Arc::clone(&storage.inner),
        }
    }

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
            values: visitor.into_inner(),
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
            self.lock().on_record(idx, visitor.into_inner());
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
