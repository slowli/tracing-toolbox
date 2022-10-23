//! `TracingEvent` consumer.

use tracing_core::{
    dispatcher,
    field::{FieldSet, Value, ValueSet},
    span::{Attributes, Id, Record},
    Event, Field, Metadata,
};

use std::{collections::HashMap, error, fmt};

use crate::{arena::ARENA, MetadataId, RawSpanId, TracedValue, TracingEvent};

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
    fn metadata(&self, id: MetadataId) -> &'static Metadata<'static> {
        self.metadata[&id]
    }

    fn map_span_id(&self, remote_id: RawSpanId) -> &Id {
        &self.span_info[&remote_id].local_id
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

    fn on_new_call_site(&mut self, id: MetadataId, data: CallSiteData, register: bool) {
        let metadata = ARENA.alloc_metadata(data);
        self.metadata.insert(id, metadata);
        if register {
            dispatcher::get_default(|dispatch| dispatch.register_callsite(metadata));
        }
    }

    pub fn consume_event(&mut self, event: TracingEvent) {
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
