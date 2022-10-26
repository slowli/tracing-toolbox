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
//! # Crate features
//!
//! Each of the three major features outlined above is gated by the corresponding opt-in feature.
//!
//! ## `subscriber`
//!
//! *(Off by default)*
//!
//! Provides [`EmittingSubscriber`].
//!
//! ## `consumer`
//!
//! *(Off by default)*
//!
//! Provides [`EventConsumer`].
//!
//! ## `capture`
//!
//! *(Off by default)*
//!
//! Provides the [`capture`](crate::capture) module, in particular [`CaptureLayer`].
//!
//! [`tardigrade`]: https://docs.rs/tardigrade
//! [tracing]: https://docs.rs/tracing/0.1/tracing
//! [`Subscriber`]: tracing_core::Subscriber
//! [the Tardigrade runtime]: https://docs.rs/tardigrade-rt
//! [`CaptureLayer`]: crate::capture::CaptureLayer

// Documentation settings.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/tardigrade-tracing/0.1.0")]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

#[cfg(feature = "capture")]
#[cfg_attr(docsrs, doc(cfg(feature = "capture")))]
pub mod capture;
#[cfg(feature = "consumer")]
#[cfg_attr(docsrs, doc(cfg(feature = "consumer")))]
mod consumer;
mod serde_helpers;
#[cfg(feature = "subscriber")]
#[cfg_attr(docsrs, doc(cfg(feature = "subscriber")))]
mod subscriber;
mod types;

#[cfg(feature = "consumer")]
pub use crate::consumer::{ConsumeError, EventConsumer, PersistedMetadata, PersistedSpans};
#[cfg(feature = "subscriber")]
pub use crate::subscriber::EmittingSubscriber;
pub use crate::types::{
    CallSiteData, CallSiteKind, DebugObject, MetadataId, RawSpanId, TracedError, TracedValue,
    TracingEvent, TracingLevel,
};
