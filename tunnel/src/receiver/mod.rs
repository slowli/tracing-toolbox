//! `TracingEvent` consumer.

use serde::{Deserialize, Serialize};
use tracing_core::{
    dispatcher::{self, Dispatch},
    field::{self, FieldSet, Value, ValueSet},
    span::{Attributes, Id, Record},
    Event, Field, Metadata,
};

use std::{collections::HashMap, error, fmt};

mod arena;
#[cfg(test)]
mod tests;

use self::arena::ARENA;
use crate::{
    serde_helpers, CallSiteData, MetadataId, RawSpanId, TracedValue, TracedValues, TracingEvent,
};

enum CowValue<'a> {
    Borrowed(&'a dyn Value),
    Owned(Box<dyn Value + 'a>),
}

impl fmt::Debug for CowValue<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Borrowed(_) => formatter.debug_struct("Borrowed").finish_non_exhaustive(),
            Self::Owned(_) => formatter.debug_struct("Owned").finish_non_exhaustive(),
        }
    }
}

impl<'a> CowValue<'a> {
    fn as_ref(&self) -> &(dyn Value + 'a) {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned(boxed) => boxed.as_ref(),
        }
    }
}

impl TracedValue {
    fn as_value(&self) -> CowValue<'_> {
        CowValue::Borrowed(match self {
            Self::Bool(value) => value,
            Self::Int(value) => value,
            Self::UInt(value) => value,
            Self::Float(value) => value,
            Self::String(value) => value,
            Self::Object(value) => return CowValue::Owned(Box::new(field::debug(value))),
            Self::Error(err) => {
                let err = err as &(dyn error::Error + 'static);
                return CowValue::Owned(Box::new(err));
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpanInfo {
    #[serde(with = "serde_helpers::span_id")]
    local_id: Id,
    metadata_id: MetadataId,
    ref_count: usize,
}

/// Information about span / event [`Metadata`] that is [serializable] and thus
/// can be persisted across multiple [`TracingEventReceiver`] lifetimes.
///
/// `PersistedMetadata` logically corresponds to a program executable (i.e., a workflow module
/// in Tardigrade), not to its particular invocation (i.e., a workflow instance in Tardigrade).
/// Multiple invocations of the same executable can (and optimally should)
/// share `PersistedMetadata`.
///
/// [serializable]: https://docs.rs/serde/1/serde
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersistedMetadata {
    inner: HashMap<MetadataId, CallSiteData>,
    // Was this metadata injected into the `tracing` runtime? This should happen the first
    // time the `PersistedMetadata` is used.
    #[serde(skip, default)]
    is_injected: bool,
}

impl PersistedMetadata {
    /// Iterates over contained call site metadata together with the corresponding
    /// [`MetadataId`]s.
    pub fn iter(&self) -> impl Iterator<Item = (MetadataId, &CallSiteData)> + '_ {
        self.inner.iter().map(|(id, data)| (*id, data))
    }
}

/// Information about alive tracing spans that is (de)serializable and thus
/// can be persisted across multiple [`TracingEventReceiver`] lifetimes.
///
/// Unlike [`PersistedMetadata`], `PersistedSpans` are specific to an executable invocation
/// (i.e., a workflow instance in Tardigrade).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersistedSpans {
    inner: HashMap<RawSpanId, SpanInfo>,
    // Were these spans injected into the `tracing` runtime? This should happen the first
    // time the `PersistedSpans` are used.
    #[serde(skip, default)]
    is_injected: bool,
}

/// Error processing a [`TracingEvent`] by a [`TracingEventReceiver`].
#[derive(Debug)]
#[non_exhaustive]
pub enum ReceiveError {
    /// The event contains a reference to an unknown [`Metadata`] ID.
    UnknownMetadataId(MetadataId),
    /// The event contains a reference to an unknown span ID.
    UnknownSpanId(RawSpanId),
    /// The event contains too many values.
    TooManyValues {
        /// Maximum supported number of values per event.
        max: usize,
        /// Actual number of values.
        actual: usize,
    },
}

impl fmt::Display for ReceiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMetadataId(id) => write!(formatter, "unknown metadata ID: {id}"),
            Self::UnknownSpanId(id) => write!(formatter, "unknown span ID: {id}"),
            Self::TooManyValues { max, actual } => write!(
                formatter,
                "too many values provided ({actual}), should be no more than {max}"
            ),
        }
    }
}

impl error::Error for ReceiveError {}

macro_rules! create_value_set {
    ($fields:ident, $values:ident, [$($i:expr,)+]) => {
        match $values.len() {
            0 => $fields.value_set(&[]),
            $(
            $i => $fields.value_set(<&[_; $i]>::try_from($values).unwrap()),
            )+
            _ => unreachable!(),
        }
    };
}

/// Receiver of [`TracingEvent`]s produced by [`TracingEventSender`] that relays them
/// to the tracing infrastructure.
///
/// The consumer takes care of persisting [`Metadata`] / spans that can outlive
/// the lifetime of the host program (not just the `TracingEventReceiver` instance!).
/// As an example, in [the Tardigrade runtime], a consumer instance is created each time
/// a workflow is executed. It relays tracing events from the workflow logic (executed in WASM)
/// to the host.
///
/// # ⚠ Resource consumption
///
/// To fit the API of the [`tracing-core`] crate, the receiver leaks string parts
/// of [`CallSiteData`]: we need a `&'static str` when we only have a `String`. Steps are taken
/// to limit the amount of leaked memory; we use a `static` string arena which checks whether
/// a particular string was already leaked, and reuses the existing `&'static str` if possible.
/// Still, this has negative implications regarding both memory consumption and performance,
/// so you probably should limit the number of executables to use with a `TracingEventReceiver`.
/// The number of *executions* of each executable is not a limiting factor.
///
/// # Examples
///
/// See [crate-level docs](index.html) for an example of usage.
///
/// [`TracingEventSender`]: crate::TracingEventSender
/// [the Tardigrade runtime]: https://docs.rs/tardigrade-rt/
/// [`tracing-core`]: https://docs.rs/tracing-core/
#[derive(Debug, Default)]
pub struct TracingEventReceiver {
    metadata: HashMap<MetadataId, &'static Metadata<'static>>,
    spans: HashMap<RawSpanId, SpanInfo>,
}

impl TracingEventReceiver {
    /// Maximum supported number of values in a span or event.
    const MAX_VALUES: usize = 32;

    /// Restores the consumer from the persisted `metadata` and tracing `spans`.
    pub fn new(metadata: &mut PersistedMetadata, spans: &mut PersistedSpans) -> Self {
        let mut this = Self::default();

        for (&id, data) in &metadata.inner {
            this.on_new_call_site(id, data.clone(), !metadata.is_injected);
        }
        metadata.is_injected = true;

        this.spans = spans.inner.clone();
        spans.is_injected = true; // FIXME: handle span registration
        this
    }

    fn dispatch<T>(dispatch_fn: impl FnOnce(&Dispatch) -> T) -> T {
        dispatch_fn(&dispatcher::get_default(Dispatch::clone))
    }

    fn metadata(&self, id: MetadataId) -> Result<&'static Metadata<'static>, ReceiveError> {
        self.metadata
            .get(&id)
            .copied()
            .ok_or(ReceiveError::UnknownMetadataId(id))
    }

    fn map_span_id(&self, remote_id: RawSpanId) -> Result<&Id, ReceiveError> {
        self.spans
            .get(&remote_id)
            .map(|span| &span.local_id)
            .ok_or(ReceiveError::UnknownSpanId(remote_id))
    }

    fn ensure_values_len(values: &TracedValues<String>) -> Result<(), ReceiveError> {
        if values.len() > Self::MAX_VALUES {
            return Err(ReceiveError::TooManyValues {
                actual: values.len(),
                max: Self::MAX_VALUES,
            });
        }
        Ok(())
    }

    fn generate_fields<'a>(
        metadata: &'static Metadata<'static>,
        values: &'a TracedValues<String>,
    ) -> Vec<(Field, CowValue<'a>)> {
        let fields = metadata.fields();
        values
            .iter()
            .map(|(field_name, value)| (fields.field(field_name).unwrap(), value.as_value()))
            .collect()
    }

    fn expand_fields<'a>(
        values: &'a [(Field, CowValue<'_>)],
    ) -> Vec<(&'a Field, Option<&'a dyn Value>)> {
        values
            .iter()
            .map(|(field, value)| (field, Some(value.as_ref())))
            .collect()
    }

    fn create_values<'a>(
        fields: &'a FieldSet,
        values: &'a [(&Field, Option<&dyn Value>)],
    ) -> ValueSet<'a> {
        create_value_set!(
            fields,
            values,
            [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24, 25, 26, 27, 28, 29, 30, 31, 32,
            ]
        )
    }

    fn on_new_call_site(&mut self, id: MetadataId, data: CallSiteData, register: bool) {
        let metadata = ARENA.alloc_metadata(data);
        self.metadata.insert(id, metadata);
        if register {
            Self::dispatch(|dispatch| dispatch.register_callsite(metadata));
        }
    }

    /// Tries to consume an event and relays it to the tracing infrastructure.
    ///
    /// # Errors
    ///
    /// Fails if the event contains a bogus reference to a call site or a span, or if it contains
    /// too many values. In general, an error can mean that the consumer was restored
    /// from an incorrect persisted state, or that the event generator is bogus (e.g.,
    /// not a [`TracingEventSender`]).
    ///
    /// [`TracingEventSender`]: crate::TracingEventSender
    #[allow(clippy::missing_panics_doc)] // false positive
    pub fn try_receive(&mut self, event: TracingEvent) -> Result<(), ReceiveError> {
        match event {
            TracingEvent::NewCallSite { id, data } => {
                self.on_new_call_site(id, data, true);
            }

            TracingEvent::NewSpan {
                id,
                parent_id,
                metadata_id,
                values,
            } => {
                Self::ensure_values_len(&values)?;

                let metadata = self.metadata(metadata_id)?;
                let values = Self::generate_fields(metadata, &values);
                let values = Self::expand_fields(&values);
                let values = Self::create_values(metadata.fields(), &values);
                let attributes = if let Some(parent_id) = parent_id {
                    let local_parent_id = self.map_span_id(parent_id)?;
                    Attributes::child_of(local_parent_id.clone(), metadata, &values)
                } else {
                    Attributes::new(metadata, &values)
                };

                // If the dispatcher is gone, we'll just stop recording any spans.
                let local_id = Self::dispatch(|dispatch| dispatch.new_span(&attributes));
                self.spans.insert(
                    id,
                    SpanInfo {
                        local_id,
                        metadata_id,
                        ref_count: 1,
                    },
                );
            }

            TracingEvent::FollowsFrom { id, follows_from } => {
                let local_id = self.map_span_id(id)?;
                let local_follows_from = self.map_span_id(follows_from)?;
                Self::dispatch(|dispatch| {
                    dispatch.record_follows_from(local_id, local_follows_from);
                });
            }
            TracingEvent::SpanEntered { id } => {
                let local_id = self.map_span_id(id)?;
                Self::dispatch(|dispatch| dispatch.enter(local_id));
            }
            TracingEvent::SpanExited { id } => {
                let local_id = self.map_span_id(id)?;
                Self::dispatch(|dispatch| dispatch.exit(local_id));
            }
            TracingEvent::SpanCloned { id } => {
                let span = self
                    .spans
                    .get_mut(&id)
                    .ok_or(ReceiveError::UnknownSpanId(id))?;
                span.ref_count += 1;
                // Dispatcher is intentionally not called: we handle ref counting locally.
            }
            TracingEvent::SpanDropped { id } => {
                let span = self
                    .spans
                    .get_mut(&id)
                    .ok_or(ReceiveError::UnknownSpanId(id))?;
                span.ref_count -= 1;
                if span.ref_count == 0 {
                    let local_id = self.spans.remove(&id).unwrap().local_id;
                    Self::dispatch(|dispatch| dispatch.try_close(local_id.clone()));
                }
            }

            TracingEvent::ValuesRecorded { id, values } => {
                Self::ensure_values_len(&values)?;

                let local_id = self.map_span_id(id)?;
                let metadata = self.metadata(self.spans[&id].metadata_id)?;
                let values = Self::generate_fields(metadata, &values);
                let values = Self::expand_fields(&values);
                let values = Self::create_values(metadata.fields(), &values);
                let values = Record::new(&values);
                Self::dispatch(|dispatch| dispatch.record(local_id, &values));
            }

            TracingEvent::NewEvent {
                metadata_id,
                parent,
                values,
            } => {
                Self::ensure_values_len(&values)?;

                let metadata = self.metadata(metadata_id)?;
                let values = Self::generate_fields(metadata, &values);
                let values = Self::expand_fields(&values);
                let values = Self::create_values(metadata.fields(), &values);
                let parent = parent.map(|id| self.map_span_id(id)).transpose()?.cloned();
                let event = if let Some(parent) = parent {
                    Event::new_child_of(parent, metadata, &values)
                } else {
                    Event::new(metadata, &values)
                };
                Self::dispatch(|dispatch| dispatch.event(&event));
            }
        }
        Ok(())
    }

    /// Consumes an event and relays it to the tracing infrastructure.
    ///
    /// # Panics
    ///
    /// Panics in the same cases when [`Self::try_receive()`] returns an error.
    pub fn receive(&mut self, event: TracingEvent) {
        self.try_receive(event)
            .expect("received bogus tracing event");
    }

    /// Persists [`Metadata`] produced by the previously consumed events. `persisted`
    /// should *logically* be the same metadata as provided to [`Self::new()`]; i.e.,
    /// metadata for a particular executable, such as a Tardigrade module.
    pub fn persist_metadata(&self, persisted: &mut PersistedMetadata) {
        persisted.is_injected = true;
        for (&id, &metadata) in &self.metadata {
            persisted
                .inner
                .entry(id)
                .or_insert_with(|| CallSiteData::from(metadata));
        }
    }

    /// Returns alive spans produced by the previously consumed events.
    pub fn persist_spans(self) -> PersistedSpans {
        PersistedSpans {
            inner: self.spans,
            is_injected: true,
        }
    }
}
