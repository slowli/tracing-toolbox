//! Types to carry tracing events over the WASM client-host boundary.

use linked_hash_map::LinkedHashMap;
use serde::{Deserialize, Serialize};
use tracing_core::{field::Visit, Field, Level, Metadata};

use std::{borrow::Cow, error, fmt, hash::Hash};

/// ID of a tracing [`Metadata`] record as used in [`TracingEvent`]s.
pub type MetadataId = u64;
/// ID of a tracing span as used in [`TracingEvent`]s.
pub type RawSpanId = u64;

/// Tracing level defined in [`CallSiteData`].
///
/// This corresponds to [`Level`] from the `tracing-core` library, but is (de)serializable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracingLevel {
    /// "ERROR" level.
    Error,
    /// "WARN" level.
    Warn,
    /// "INFO" level.
    Info,
    /// "DEBUG" level.
    Debug,
    /// "TRACE" level.
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

/// Kind of [`CallSiteData`] location: either a span, or an event.
#[derive(Debug, Clone, Copy, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallSiteKind {
    /// Call site is a span.
    Span,
    /// Call site is an event.
    Event,
}

/// Data for a single tracing call site: either a span definition, or an event definition.
///
/// This corresponds to [`Metadata`] from the `tracing-core` library, but is (de)serializable.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct CallSiteData {
    /// Kind of the call site.
    pub kind: CallSiteKind,
    /// Name of the call site.
    pub name: Cow<'static, str>,
    /// Tracing target.
    pub target: Cow<'static, str>,
    /// Tracing level.
    pub level: TracingLevel,
    /// Path to the module where this call site is defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_path: Option<Cow<'static, str>>,
    /// Path to the file where this call site is defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<Cow<'static, str>>,
    /// Line number for this call site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Fields defined by this call site.
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

/// Event produced during tracing.
///
/// These events are emitted by a [`TracingEventSender`] and then consumed
/// by a [`TracingEventReceiver`] to pass tracing info across an API boundary.
///
/// [`TracingEventSender`]: crate::TracingEventSender
/// [`TracingEventReceiver`]: crate::TracingEventReceiver
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TracingEvent {
    /// New call site.
    NewCallSite {
        /// Unique ID of the call site that will be used to refer to it in the following events.
        id: MetadataId,
        /// Information about the call site.
        #[serde(flatten)]
        data: CallSiteData,
    },

    /// New tracing span.
    NewSpan {
        /// Unique ID of the span that will be used to refer to it in the following events.
        id: RawSpanId,
        /// Parent span ID. `None` means using the contextual parent (i.e., the current span).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<RawSpanId>,
        /// ID of the span metadata.
        metadata_id: MetadataId,
        /// Values associated with the span.
        values: TracedValues<String>,
    },
    /// New "follows from" relation between spans.
    FollowsFrom {
        /// ID of the follower span.
        id: RawSpanId,
        /// ID of the source span.
        follows_from: RawSpanId,
    },
    /// Span was entered.
    SpanEntered {
        /// ID of the span.
        id: RawSpanId,
    },
    /// Span was exited.
    SpanExited {
        /// ID of the span.
        id: RawSpanId,
    },
    /// Span was cloned.
    SpanCloned {
        /// ID of the span.
        id: RawSpanId,
    },
    /// Span was dropped (aka closed).
    SpanDropped {
        /// ID of the span.
        id: RawSpanId,
    },
    /// New values recorded for a span.
    ValuesRecorded {
        /// ID of the span.
        id: RawSpanId,
        /// Recorded values.
        values: TracedValues<String>,
    },

    /// New event.
    NewEvent {
        /// ID of the event metadata.
        metadata_id: MetadataId,
        /// Parent span ID. `None` means using the contextual parent (i.e., the current span).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<RawSpanId>,
        /// Values associated with the event.
        values: TracedValues<String>,
    },
}

/// (De)serializable presentation for an error recorded as a value in a tracing span or event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TracedError {
    /// Error message produced by its [`Display`](fmt::Display) implementation.
    pub message: String,
    /// Error [source](error::Error::source()).
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

/// Opaque wrapper for a [`Debug`](fmt::Debug)gable object recorded as a value
/// in a tracing span or event.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DebugObject(String);

impl fmt::Debug for DebugObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Returns the [`Debug`](fmt::Debug) representation of the object.
impl AsRef<str> for DebugObject {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Value recorded in a tracing span or event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TracedValue {
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    Int(i128),
    /// Unsigned integer value.
    UInt(u128),
    /// Floating-point value.
    Float(f64),
    /// String value.
    String(String),
    /// Opaque object implementing the [`Debug`](fmt::Debug) trait.
    Object(DebugObject),
    /// Opaque error.
    Error(TracedError),
}

macro_rules! impl_value_conversions {
    (TracedValue :: $variant:ident ($source:ty)) => {
        impl From<$source> for TracedValue {
            fn from(value: $source) -> Self {
                Self::$variant(value)
            }
        }

        impl PartialEq<$source> for TracedValue {
            fn eq(&self, other: &$source) -> bool {
                match self {
                    Self::$variant(value) => value == other,
                    _ => false,
                }
            }
        }

        impl PartialEq<TracedValue> for $source {
            fn eq(&self, other: &TracedValue) -> bool {
                other == self
            }
        }
    };

    (TracedValue :: $variant:ident ($source:ty as $field_ty:ty)) => {
        impl From<$source> for TracedValue {
            fn from(value: $source) -> Self {
                Self::$variant(value.into())
            }
        }

        impl PartialEq<$source> for TracedValue {
            fn eq(&self, other: &$source) -> bool {
                match self {
                    Self::$variant(value) => *value == <$field_ty>::from(*other),
                    _ => false,
                }
            }
        }

        impl PartialEq<TracedValue> for $source {
            fn eq(&self, other: &TracedValue) -> bool {
                other == self
            }
        }
    };
}

impl_value_conversions!(TracedValue::Bool(bool));
impl_value_conversions!(TracedValue::Int(i128));
impl_value_conversions!(TracedValue::Int(i64 as i128));
impl_value_conversions!(TracedValue::UInt(u128));
impl_value_conversions!(TracedValue::UInt(u64 as u128));
impl_value_conversions!(TracedValue::Float(f64));

impl PartialEq<str> for TracedValue {
    fn eq(&self, other: &str) -> bool {
        match self {
            Self::String(value) => value == other,
            _ => false,
        }
    }
}

impl PartialEq<TracedValue> for str {
    fn eq(&self, other: &TracedValue) -> bool {
        other == self
    }
}

impl From<&str> for TracedValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl PartialEq<&str> for TracedValue {
    fn eq(&self, other: &&str) -> bool {
        match self {
            Self::String(value) => value == *other,
            _ => false,
        }
    }
}

impl PartialEq<TracedValue> for &str {
    fn eq(&self, other: &TracedValue) -> bool {
        other == self
    }
}

impl TracedValue {
    fn debug(object: &dyn fmt::Debug) -> Self {
        Self::Object(DebugObject(format!("{object:?}")))
    }

    /// Returns value as a Boolean, or `None` if it's not a Boolean value.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns value as a signed integer, or `None` if it's not one.
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns value as an unsigned integer, or `None` if it's not one.
    pub fn as_uint(&self) -> Option<u128> {
        match self {
            Self::UInt(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns value as a floating-point value, or `None` if it's not one.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns value as a string, or `None` if it's not one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Checks whether this value is a [`DebugObject`] with the same [`Debug`](fmt::Debug)
    /// output as the provided `object`.
    pub fn is_debug(&self, object: &dyn fmt::Debug) -> bool {
        match self {
            Self::Object(value) => value.0 == format!("{object:?}"),
            _ => false,
        }
    }

    /// Returns value as a [`Debug`](fmt::Debug) string output, or `None` if this value
    /// is not [`Self::Object`].
    pub fn as_debug_str(&self) -> Option<&str> {
        match self {
            Self::Object(value) => Some(&value.0),
            _ => None,
        }
    }

    fn error(err: &(dyn error::Error + 'static)) -> Self {
        Self::Error(TracedError::new(err))
    }
}

/// Collection of named [`TracedValue`]s.
pub type TracedValues<K> = LinkedHashMap<K, TracedValue>;

#[doc(hidden)] // not public; used by `tracing-capture`
pub struct ValueVisitor<S> {
    pub values: TracedValues<S>,
}

impl<S: fmt::Debug + Eq + Hash> fmt::Debug for ValueVisitor<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValueVisitor")
            .field("values", &self.values)
            .finish()
    }
}

impl<S: Eq + Hash> Default for ValueVisitor<S> {
    fn default() -> Self {
        Self {
            values: TracedValues::new(),
        }
    }
}

impl<S: From<&'static str> + Eq + Hash> Visit for ValueVisitor<S> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.values.insert(field.name().into(), value.into());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn error::Error + 'static)) {
        self.values
            .insert(field.name().into(), TracedValue::error(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.values
            .insert(field.name().into(), TracedValue::debug(value));
    }
}
