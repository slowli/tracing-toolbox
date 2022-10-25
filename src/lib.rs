//! Tracing infrastructure for the Tardigrade orchestration engine.
//!
//! This crate provides various [tracing] infrastructure helpers for [`tardigrade`]
//! workflows.
//!
//! - [`EmittingSubscriber`] is a tracing [`Subscriber`] that converts tracing events
//!   into (de)serializable presentation that can be sent elsewhere using a customizable hook.
//!   The [`tardigrade`] client library uses this subscriber to send tracing events to the host.
//! - [`EventConsumer`] consumes events produced by `EmittingSubscriber` and relays them
//!   to the tracing infrastructure. The consumer is used by [the Tardigrade runtime].
//! - [`CaptureLayer`] can be used to capture spans during testing.
//!
//! [`tardigrade`]: https://docs.rs/tardigrade
//! [tracing]: https://docs.rs/tracing/0.1/tracing
//! [`Subscriber`]: tracing_core::Subscriber
//! [the Tardigrade runtime]: https://docs.rs/tardigrade-rt
//! [`CaptureLayer`]: crate::capture::CaptureLayer

// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

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
