//! Tracing infrastructure for Tardigrade orchestration engine.

mod arena;
mod consumer;
mod serde_helpers;
mod subscriber;
mod types;

pub use crate::{
    consumer::{EventConsumer, PersistedMetadata, PersistedSpans},
    subscriber::EmittingSubscriber,
    types::{
        CallSiteData, CallSiteKind, MetadataId, RawSpanId, TracedError, TracedValue, TracingEvent,
        TracingLevel,
    },
};
