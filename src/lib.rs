//! Tracing infrastructure for Tardigrade orchestration engine.

#[cfg(feature = "capture")]
pub mod capture;
#[cfg(feature = "consumer")]
mod consumer;
mod serde_helpers;
#[cfg(feature = "subscriber")]
mod subscriber;
mod types;

#[cfg(feature = "consumer")]
pub use crate::consumer::{EventConsumer, PersistedMetadata, PersistedSpans};
#[cfg(feature = "subscriber")]
pub use crate::subscriber::EmittingSubscriber;
pub use crate::types::{
    CallSiteData, CallSiteKind, MetadataId, RawSpanId, TracedError, TracedValue, TracingEvent,
    TracingLevel,
};
