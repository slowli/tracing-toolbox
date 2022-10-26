//! Tracing infrastructure for the Tardigrade orchestration engine.
//!
//! This crate provides various [tracing] infrastructure helpers for [`tardigrade`]
//! workflows.
//!
//! - [`TracingEventSender`] is a tracing [`Subscriber`] that converts tracing events
//!   into (de)serializable presentation that can be sent elsewhere using a customizable hook.
//!   The [`tardigrade`] client library uses this subscriber to send tracing events to the host.
//! - [`TracingEventReceiver`] consumes events produced by a `TracingEventSender` and relays them
//!   to the tracing infrastructure. The receiver is used by [the Tardigrade runtime].
//! - [`CaptureLayer`] can be used to capture spans during testing.
//!
//! # Crate features
//!
//! Each of the three major features outlined above is gated by the corresponding opt-in feature.
//!
//! ## `sender`
//!
//! *(Off by default)*
//!
//! Provides [`TracingEventSender`].
//!
//! ## `receiver`
//!
//! *(Off by default)*
//!
//! Provides [`TracingEventReceiver`].
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
#[cfg(feature = "receiver")]
#[cfg_attr(docsrs, doc(cfg(feature = "receiver")))]
mod receiver;
#[cfg(feature = "sender")]
#[cfg_attr(docsrs, doc(cfg(feature = "sender")))]
mod sender;
mod serde_helpers;
mod types;

#[cfg(feature = "receiver")]
pub use crate::receiver::{PersistedMetadata, PersistedSpans, ReceiveError, TracingEventReceiver};
#[cfg(feature = "sender")]
pub use crate::sender::TracingEventSender;
pub use crate::types::{
    CallSiteData, CallSiteKind, DebugObject, MetadataId, RawSpanId, TracedError, TracedValue,
    TracingEvent, TracingLevel,
};
