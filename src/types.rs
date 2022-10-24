//! Types to carry tracing events over the WASM client-host boundary.

use serde::{Deserialize, Serialize};
use tracing_core::{
    field::Visit,
    span::{Attributes, Id, Record},
    Event, Field, Level, Metadata,
};

use std::{borrow::Cow, error, fmt};

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

impl CallSiteData {
    pub(crate) fn new(metadata: &'static Metadata<'static>) -> Self {
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
            level: TracingLevel::new(metadata.level()),
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
        #[serde(with = "serde_tuples")]
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
        #[serde(with = "serde_tuples")]
        values: Vec<(String, TracedValue)>,
    },

    NewEvent {
        metadata_id: MetadataId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<RawSpanId>,
        #[serde(with = "serde_tuples")]
        values: Vec<(String, TracedValue)>,
    },
}

mod serde_tuples {
    use serde::{
        de::{MapAccess, Visitor},
        ser::SerializeMap,
        Deserialize, Deserializer, Serialize, Serializer,
    };

    use std::{fmt, marker::PhantomData};

    pub fn serialize<S: Serializer, T: Serialize>(
        tuples: &[(String, T)],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(tuples.len()))?;
        for (name, value) in tuples {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<Vec<(String, T)>, D::Error> {
        struct MapVisitor<T> {
            _ty: PhantomData<T>,
        }

        impl<T> Default for MapVisitor<T> {
            fn default() -> Self {
                Self { _ty: PhantomData }
            }
        }

        impl<'de, T: Deserialize<'de>> Visitor<'de> for MapVisitor<T> {
            type Value = Vec<(String, T)>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("map of name-value pairs")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = map.size_hint().map_or_else(Vec::new, Vec::with_capacity);
                while let Some(tuple) = map.next_entry::<String, T>()? {
                    entries.push(tuple);
                }
                Ok(entries)
            }
        }

        deserializer.deserialize_map(MapVisitor::<T>::default())
    }
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
    FloatingPoint(f64),
    String(String),
    Object(DebugObject),
    Error(TracedError),
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
        Self::Object(DebugObject(format!("{object:?}")))
    }

    fn error(err: &(dyn error::Error + 'static)) -> Self {
        Self::Error(TracedError::new(err))
    }
}

impl TracingEvent {
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
    values: Vec<(String, TracedValue)>,
}

impl Visit for ValueVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.values.push((field.name().to_owned(), value.into()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.values.push((field.name().to_owned(), value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values.push((field.name().to_owned(), value.into()));
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.values.push((field.name().to_owned(), value.into()));
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.values.push((field.name().to_owned(), value.into()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.values.push((field.name().to_owned(), value.into()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.values.push((field.name().to_owned(), value.into()));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn error::Error + 'static)) {
        self.values
            .push((field.name().to_owned(), TracedValue::error(value)));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.values
            .push((field.name().to_owned(), TracedValue::debug(value)));
    }
}
