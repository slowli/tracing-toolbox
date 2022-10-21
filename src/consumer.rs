//! `TracingEvent` consumer.

use once_cell::sync::OnceCell;
use tracing_core::{
    dispatcher,
    field::{FieldSet, Value, ValueSet},
    span::{Attributes, Id, Record},
    Callsite, Event, Field, Interest, Kind, Level, Metadata,
};

use std::{borrow::Cow, collections::HashMap, error, fmt};

use crate::{CallSiteKind, MetadataId, RawSpanId, TracedValue, TracingEvent, TracingLevel};

impl From<TracingLevel> for Level {
    fn from(level: TracingLevel) -> Self {
        match level {
            TracingLevel::Error => Self::ERROR,
            TracingLevel::Warn => Self::WARN,
            TracingLevel::Info => Self::INFO,
            TracingLevel::Debug => Self::DEBUG,
            TracingLevel::Trace => Self::TRACE,
        }
    }
}

impl From<CallSiteKind> for Kind {
    fn from(kind: CallSiteKind) -> Self {
        match kind {
            CallSiteKind::Span => Self::SPAN,
            CallSiteKind::Event => Self::EVENT,
        }
    }
}

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
            Self::FloatingPoint(value) => value,
            Self::String(value) | Self::Object(value) => value,
            Self::Error(err) => {
                let err = err as &(dyn error::Error + 'static);
                return CowValue::Owned(Box::new(err));
            }
        })
    }
}

#[derive(Debug, Default)]
struct DynamicCallSite {
    metadata: OnceCell<&'static Metadata<'static>>,
}

impl Callsite for DynamicCallSite {
    fn set_interest(&self, _interest: Interest) {
        // Does nothing
    }

    fn metadata(&self) -> &Metadata<'_> {
        self.metadata
            .get()
            .copied()
            .expect("metadata not initialized")
    }
}

#[derive(Debug)]
struct SpanInfo {
    local_id: Id,
    metadata_id: MetadataId,
    ref_count: usize,
}

#[derive(Debug, Default)]
pub struct EventConsumer {
    metadata: HashMap<MetadataId, &'static Metadata<'static>>,
    span_info: HashMap<RawSpanId, SpanInfo>,
}

impl EventConsumer {
    fn insert_metadata(
        &mut self,
        id: MetadataId,
        metadata: Metadata<'static>,
    ) -> &'static Metadata<'static> {
        let metadata = Box::leak(Box::new(metadata)) as &'static _;
        self.metadata.insert(id, metadata);
        metadata
    }

    fn metadata(&self, id: MetadataId) -> &'static Metadata<'static> {
        self.metadata[&id]
    }

    fn map_span_id(&self, remote_id: RawSpanId) -> &Id {
        &self.span_info[&remote_id].local_id
    }

    // FIXME: use arena to deduplicate
    fn leak(s: Cow<'static, str>) -> &'static str {
        match s {
            Cow::Borrowed(s) => s,
            Cow::Owned(string) => Box::leak(string.into_boxed_str()),
        }
    }

    fn leak_fields(fields: Vec<Cow<'static, str>>) -> &'static [&'static str] {
        let fields: Box<[_]> = fields.into_iter().map(Self::leak).collect();
        Box::leak(fields)
    }

    fn new_call_site() -> &'static DynamicCallSite {
        let call_site = Box::new(DynamicCallSite::default());
        Box::leak(call_site)
    }

    fn generate_fields<'a>(
        metadata: &'static Metadata<'static>,
        values: &'a [(String, TracedValue)],
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
        match values.len() {
            0 => fields.value_set(&[]),
            1 => fields.value_set(<&[_; 1]>::try_from(values).unwrap()),
            2 => fields.value_set(<&[_; 2]>::try_from(values).unwrap()),
            3 => fields.value_set(<&[_; 3]>::try_from(values).unwrap()),
            4 => fields.value_set(<&[_; 4]>::try_from(values).unwrap()),
            _ => todo!(),
        }
    }

    pub fn consume_event(&mut self, event: TracingEvent) {
        match event {
            TracingEvent::NewCallSite { kind, id, data } => {
                let call_site = Self::new_call_site();
                let call_site_id = tracing_core::identify_callsite!(call_site);
                let fields = FieldSet::new(Self::leak_fields(data.fields), call_site_id);
                let metadata = Metadata::new(
                    Self::leak(data.name),
                    Self::leak(data.target),
                    data.level.into(),
                    data.file.map(Self::leak),
                    data.line,
                    data.module_path.map(Self::leak),
                    fields,
                    kind.into(),
                );
                let metadata = self.insert_metadata(id, metadata);
                call_site.metadata.set(metadata).unwrap();

                dispatcher::get_default(|dispatch| dispatch.register_callsite(metadata));
            }

            TracingEvent::NewSpan {
                id,
                parent_id,
                metadata_id,
                values,
            } => {
                let metadata = self.metadata(metadata_id);
                let values = Self::generate_fields(metadata, &values);
                let values = Self::expand_fields(&values);
                let values = Self::create_values(metadata.fields(), &values);
                let attributes = if let Some(parent_id) = parent_id {
                    let local_parent_id = self.map_span_id(parent_id);
                    Attributes::child_of(local_parent_id.clone(), metadata, &values)
                } else {
                    Attributes::new(metadata, &values)
                };

                let local_id = dispatcher::get_default(|dispatch| dispatch.new_span(&attributes));
                self.span_info.insert(
                    id,
                    SpanInfo {
                        local_id,
                        metadata_id,
                        ref_count: 1,
                    },
                );
            }

            TracingEvent::FollowsFrom { id, follows_from } => {
                let local_id = self.map_span_id(id);
                let local_follows_from = self.map_span_id(follows_from);
                dispatcher::get_default(|dispatch| {
                    dispatch.record_follows_from(local_id, local_follows_from)
                });
            }
            TracingEvent::SpanEntered { id } => {
                let local_id = self.map_span_id(id);
                dispatcher::get_default(|dispatch| dispatch.enter(local_id));
            }
            TracingEvent::SpanExited { id } => {
                let local_id = self.map_span_id(id);
                dispatcher::get_default(|dispatch| dispatch.exit(local_id));
            }
            TracingEvent::SpanCloned { id } => {
                self.span_info.get_mut(&id).unwrap().ref_count += 1;
                // Dispatcher is intentionally not called: we handle ref counting locally.
            }
            TracingEvent::SpanDropped { id } => {
                self.span_info.get_mut(&id).unwrap().ref_count -= 1;
                if self.span_info[&id].ref_count == 0 {
                    let local_id = self.span_info.remove(&id).unwrap().local_id;
                    dispatcher::get_default(|dispatch| dispatch.try_close(local_id.clone()));
                }
            }

            TracingEvent::ValuesRecorded { id, values } => {
                let local_id = self.map_span_id(id);
                let metadata = self.metadata(self.span_info[&id].metadata_id);
                let values = Self::generate_fields(metadata, &values);
                let values = Self::expand_fields(&values);
                let values = Self::create_values(metadata.fields(), &values);
                let values = Record::new(&values);
                dispatcher::get_default(|dispatch| dispatch.record(local_id, &values))
            }

            TracingEvent::NewEvent {
                metadata_id,
                parent,
                values,
            } => {
                let metadata = self.metadata(metadata_id);
                let values = Self::generate_fields(metadata, &values);
                let values = Self::expand_fields(&values);
                let values = Self::create_values(metadata.fields(), &values);
                let parent = parent.map(|id| self.map_span_id(id).clone());
                let event = Event::new_child_of(parent, metadata, &values);
                dispatcher::get_default(|dispatch| dispatch.event(&event));
            }
        }
    }
}
