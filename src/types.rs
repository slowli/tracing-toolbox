//! Types to carry tracing events over the WASM client-host boundary.

use serde::{Deserialize, Serialize};
use tracing_core::{
    field::Visit,
    span::{Attributes, Id, Record},
    Event, Field, Level, Metadata,
};

use std::{borrow::Cow, collections::HashMap, error, fmt, iter};

pub type MetadataId = u64;
pub type RawSpanId = u64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracingLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl TracingLevel {
    fn new(level: &Level) -> Self {
        match *level {
            Level::ERROR => Self::Error,
            Level::WARN => Self::Warn,
            Level::INFO => Self::Info,
            Level::DEBUG => Self::Debug,
            Level::TRACE => Self::Trace,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallSiteKind {
    Span,
    Event,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TracingEvent {
    NewCallSite {
        kind: CallSiteKind,
        id: MetadataId,
        name: Cow<'static, str>,
        target: Cow<'static, str>,
        level: TracingLevel,
        module_path: Option<Cow<'static, str>>,
        file: Option<Cow<'static, str>>,
        line: Option<u32>,
        fields: Vec<Cow<'static, str>>,
    },

    NewSpan {
        id: RawSpanId,
        parent_id: Option<RawSpanId>,
        metadata_id: MetadataId,
        values: HashMap<String, TracedValue>,
    },
    FollowsFrom {
        id: RawSpanId,
        follows_from: RawSpanId,
    },
    SpanEntered {
        id: RawSpanId,
    },
    SpanExited {
        id: RawSpanId,
    },
    ValuesRecorded {
        id: RawSpanId,
        values: HashMap<String, TracedValue>,
    },

    NewEvent {
        metadata_id: MetadataId,
        parent: Option<RawSpanId>,
        values: HashMap<String, TracedValue>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TracedError {
    pub message: String,
}

impl TracedError {
    fn new(err: &(dyn error::Error + 'static)) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracedValue {
    Bool(bool),
    Int(i128),
    UInt(u128),
    FloatingPoint(f64),
    String(String),
    Object(String),
    Error(Vec<TracedError>),
}

impl From<bool> for TracedValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i128> for TracedValue {
    fn from(value: i128) -> Self {
        Self::Int(value)
    }
}

impl From<i64> for TracedValue {
    fn from(value: i64) -> Self {
        Self::Int(value.into())
    }
}

impl From<u128> for TracedValue {
    fn from(value: u128) -> Self {
        Self::UInt(value)
    }
}

impl From<u64> for TracedValue {
    fn from(value: u64) -> Self {
        Self::UInt(value.into())
    }
}

impl From<f64> for TracedValue {
    fn from(value: f64) -> Self {
        Self::FloatingPoint(value)
    }
}

impl From<&str> for TracedValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl TracedValue {
    fn debug(object: &dyn fmt::Debug) -> Self {
        Self::Object(format!("{object:?}"))
    }

    fn error(err: &(dyn error::Error + 'static)) -> Self {
        let error_chain = iter::successors(Some(err), |err| err.source())
            .map(TracedError::new)
            .collect();
        Self::Error(error_chain)
    }
}

impl TracingEvent {
    pub(crate) fn new_call_site(metadata: &'static Metadata<'static>, id: MetadataId) -> Self {
        let kind = if metadata.is_span() {
            CallSiteKind::Span
        } else {
            debug_assert!(metadata.is_event());
            CallSiteKind::Event
        };
        let fields = metadata
            .fields()
            .iter()
            .map(|field| Cow::Borrowed(field.name()));

        Self::NewCallSite {
            kind,
            id,
            name: Cow::Borrowed(metadata.name()),
            target: Cow::Borrowed(metadata.target()),
            level: TracingLevel::new(metadata.level()),
            module_path: metadata.module_path().map(Cow::Borrowed),
            file: metadata.file().map(Cow::Borrowed),
            line: metadata.line(),
            fields: fields.collect(),
        }
    }

    pub(crate) fn new_span(span: &Attributes<'_>, metadata_id: MetadataId, id: RawSpanId) -> Self {
        let mut visitor = ValueVisitor::default();
        span.record(&mut visitor);
        Self::NewSpan {
            id,
            parent_id: span.parent().map(Id::into_u64),
            metadata_id,
            values: visitor.values,
        }
    }

    pub(crate) fn values_recorded(id: RawSpanId, values: &Record<'_>) -> Self {
        let mut visitor = ValueVisitor::default();
        values.record(&mut visitor);
        Self::ValuesRecorded {
            id,
            values: visitor.values,
        }
    }

    pub(crate) fn new_event(event: &Event<'_>, metadata_id: MetadataId) -> Self {
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
struct ValueVisitor {
    values: HashMap<String, TracedValue>,
}

impl Visit for ValueVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn error::Error + 'static)) {
        self.values
            .insert(field.name().to_owned(), TracedValue::error(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.values
            .insert(field.name().to_owned(), TracedValue::debug(value));
    }
}
