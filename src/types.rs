//! Types to carry tracing events over the WASM client-host boundary.

use serde::{Deserialize, Serialize};
use tracing_core::{field::Visit, Field, Level, Metadata};

use std::{borrow::Cow, error, fmt};

use crate::serde_helpers;

pub type MetadataId = u64;
pub type RawSpanId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracingLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<Level> for TracingLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::ERROR => Self::Error,
            Level::WARN => Self::Warn,
            Level::INFO => Self::Info,
            Level::DEBUG => Self::Debug,
            Level::TRACE => Self::Trace,
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallSiteKind {
    Span,
    Event,
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CallSiteData {
    pub kind: CallSiteKind,
    pub name: Cow<'static, str>,
    pub target: Cow<'static, str>,
    pub level: TracingLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_path: Option<Cow<'static, str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<Cow<'static, str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub fields: Vec<Cow<'static, str>>,
}

impl From<&Metadata<'static>> for CallSiteData {
    fn from(metadata: &Metadata<'static>) -> Self {
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

        Self {
            kind,
            name: Cow::Borrowed(metadata.name()),
            target: Cow::Borrowed(metadata.target()),
            level: TracingLevel::from(*metadata.level()),
            module_path: metadata.module_path().map(Cow::Borrowed),
            file: metadata.file().map(Cow::Borrowed),
            line: metadata.line(),
            fields: fields.collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TracingEvent {
    NewCallSite {
        id: MetadataId,
        #[serde(flatten)]
        data: CallSiteData,
    },

    NewSpan {
        id: RawSpanId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<RawSpanId>,
        metadata_id: MetadataId,
        #[serde(with = "serde_helpers::tuples")]
        values: Vec<(String, TracedValue)>,
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
    SpanCloned {
        id: RawSpanId,
    },
    SpanDropped {
        id: RawSpanId,
    },
    ValuesRecorded {
        id: RawSpanId,
        #[serde(with = "serde_helpers::tuples")]
        values: Vec<(String, TracedValue)>,
    },

    NewEvent {
        metadata_id: MetadataId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<RawSpanId>,
        #[serde(with = "serde_helpers::tuples")]
        values: Vec<(String, TracedValue)>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TracedError {
    pub message: String,
    pub source: Option<Box<TracedError>>,
}

impl TracedError {
    fn new(err: &(dyn error::Error + 'static)) -> Self {
        Self {
            message: err.to_string(),
            source: err.source().map(|source| Box::new(Self::new(source))),
        }
    }
}

impl fmt::Display for TracedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl error::Error for TracedError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn error::Error + 'static))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DebugObject(String);

impl fmt::Debug for DebugObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracedValue {
    Bool(bool),
    Int(i128),
    UInt(u128),
    Float(f64),
    String(String),
    Object(DebugObject),
    Error(TracedError),
}

impl From<bool> for TracedValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl PartialEq<bool> for TracedValue {
    fn eq(&self, other: &bool) -> bool {
        match self {
            Self::Bool(value) => value == other,
            _ => false,
        }
    }
}

impl From<i128> for TracedValue {
    fn from(value: i128) -> Self {
        Self::Int(value)
    }
}

impl PartialEq<i128> for TracedValue {
    fn eq(&self, other: &i128) -> bool {
        match self {
            Self::Int(value) => value == other,
            _ => false,
        }
    }
}

impl From<i64> for TracedValue {
    fn from(value: i64) -> Self {
        Self::Int(value.into())
    }
}

impl PartialEq<i64> for TracedValue {
    fn eq(&self, other: &i64) -> bool {
        match self {
            Self::Int(value) => *value == i128::from(*other),
            _ => false,
        }
    }
}

impl From<u128> for TracedValue {
    fn from(value: u128) -> Self {
        Self::UInt(value)
    }
}

impl PartialEq<u128> for TracedValue {
    fn eq(&self, other: &u128) -> bool {
        match self {
            Self::UInt(value) => value == other,
            _ => false,
        }
    }
}

impl From<u64> for TracedValue {
    fn from(value: u64) -> Self {
        Self::UInt(value.into())
    }
}

impl PartialEq<u64> for TracedValue {
    fn eq(&self, other: &u64) -> bool {
        match self {
            Self::UInt(value) => *value == u128::from(*other),
            _ => false,
        }
    }
}

impl From<f64> for TracedValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl PartialEq<f64> for TracedValue {
    fn eq(&self, other: &f64) -> bool {
        match self {
            Self::Float(value) => value == other,
            _ => false,
        }
    }
}

impl From<&str> for TracedValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl PartialEq<str> for TracedValue {
    fn eq(&self, other: &str) -> bool {
        match self {
            Self::String(value) => value == other,
            _ => false,
        }
    }
}

impl TracedValue {
    fn debug(object: &dyn fmt::Debug) -> Self {
        Self::Object(DebugObject(format!("{object:?}")))
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i128> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_uint(&self) -> Option<u128> {
        match self {
            Self::UInt(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_debug(&self, object: &dyn fmt::Debug) -> bool {
        match self {
            Self::Object(value) => value.0 == format!("{object:?}"),
            _ => false,
        }
    }

    fn error(err: &(dyn error::Error + 'static)) -> Self {
        Self::Error(TracedError::new(err))
    }
}

#[derive(Debug, Default)]
pub(crate) struct ValueVisitor<S> {
    pub values: Vec<(S, TracedValue)>,
}

impl<S: From<&'static str>> Visit for ValueVisitor<S> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.values.push((field.name().into(), value.into()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.values.push((field.name().into(), value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values.push((field.name().into(), value.into()));
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.values.push((field.name().into(), value.into()));
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.values.push((field.name().into(), value.into()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.values.push((field.name().into(), value.into()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.values.push((field.name().into(), value.into()));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn error::Error + 'static)) {
        self.values
            .push((field.name().into(), TracedValue::error(value)));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.values
            .push((field.name().into(), TracedValue::debug(value)));
    }
}
