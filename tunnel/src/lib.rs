//! Tunnelling tracing information across an API boundary.
//!
//! This crate provides [tracing] infrastructure helpers allowing to transfer tracing events
//! across an API boundary:
//!
//! - [`TracingEventSender`] is a tracing [`Subscriber`] that converts tracing events
//!   into (de)serializable presentation that can be sent elsewhere using a customizable hook.
//! - [`TracingEventReceiver`] consumes events produced by a `TracingEventSender` and relays them
//!   to the tracing infrastructure. It is assumed that the source of events may outlive
//!   both the lifetime of a particular `TracingEventReceiver` instance, and the lifetime
//!   of the program encapsulating the receiver. To deal with this, the receiver provides
//!   the means to persist / restore its state.
//!
//! [`tardigrade`]: https://docs.rs/tardigrade
//! [The Tardigrade runtime]: https://docs.rs/tardigrade-rt
//!
//! # When is this needed?
//!
//! This crate solves the problem of having *dynamic* call sites for tracing
//! spans / events, i.e., ones not known during compilation. This may occur if call sites
//! are defined in dynamically loaded modules, the execution of which is embedded into the program,
//! e.g., WASM modules.
//!
//! It *could* be feasible to treat such a module as a separate program and
//! collect / analyze its traces in conjunction with host traces using distributed tracing software
//! (e.g., [OpenTelemetry] / [Jaeger]). However, this would significantly bloat the API surface
//! of the module, bloat its dependency tree, and would arguably break encapsulation.
//!
//! The approach proposed in this crate keeps the module API as simple as possible: essentially,
//! a single function to smuggle [`TracingEvent`]s through the client–host boundary.
//! The client side (i.e., the [`TracingEventSender`]) is almost stateless;
//! it just streams tracing events to the host, which can have tracing logic as complex as required.
//!
//! Another problem that this crate solves is having module executions that can outlive
//! the host program. For example, WASM module instances can be fully persisted and resumed later,
//! potentially after the host is restarted. To solve this, [`TracingEventReceiver`] allows
//! persisting call site data and alive spans, and resuming from the previously saved state
//! (notifying the tracing infra about call sites / spans if necessary).
//!
//! ## Use case: workflow automation
//!
//! Both components are used by the [Tardigrade][`tardigrade`] workflows, in case of which
//! the API boundary is the WASM client–host boundary.
//!
//! - The [`tardigrade`] client library uses [`TracingEventSender`] to send tracing events
//!   from a workflow (i.e., a WASM module instance) to the host using a WASM import function.
//! - [The Tardigrade runtime] uses [`TracingEventReceiver`] to pass traces from the workflow
//!   to the host tracing infrastructure.
//!
//! [tracing]: https://docs.rs/tracing/0.1/tracing
//! [`Subscriber`]: tracing_core::Subscriber
//! [OpenTelemetry]: https://opentelemetry.io/
//! [Jaeger]: https://www.jaegertracing.io/
//!
//! # Crate features
//!
//! Each of the two major features outlined above is gated by the corresponding opt-in feature.
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
//! # Examples
//!
//! ## Sending events with `TracingEventSender`
//!
//! ```
//! # use assert_matches::assert_matches;
//! # use std::sync::mpsc;
//! use tracing_tunnel::{TracingEvent, TracingEventSender, TracingEventReceiver};
//!
//! // Let's collect tracing events using an MPSC channel.
//! let (events_sx, events_rx) = mpsc::sync_channel(10);
//! let subscriber = TracingEventSender::new(move |event| {
//!     events_sx.send(event).ok();
//! });
//!
//! tracing::subscriber::with_default(subscriber, || {
//!     tracing::info_span!("test", num = 42_i64).in_scope(|| {
//!         tracing::warn!("I feel disturbance in the Force...");
//!     });
//! });
//!
//! let events: Vec<_> = events_rx.iter().collect();
//! assert!(!events.is_empty());
//! // There should be one "new span".
//! let span_count = events
//!     .iter()
//!     .filter(|event| matches!(event, TracingEvent::NewSpan { .. }))
//!     .count();
//! assert_eq!(span_count, 1);
//! ```
//!
//! ## Receiving events from `TracingEventReceiver`
//!
//! ```
//! # use tracing_tunnel::{
//! #     LocalSpans, PersistedMetadata, PersistedSpans, TracingEvent, TracingEventReceiver
//! # };
//! tracing_subscriber::fmt().pretty().init();
//!
//! let events: Vec<TracingEvent> = // ...
//! #    vec![];
//!
//! let mut spans = PersistedSpans::default();
//! let mut local_spans = LocalSpans::default();
//! // Replay `events` using the default subscriber.
//! let mut receiver = TracingEventReceiver::new(
//!     PersistedMetadata::default(),
//!     &mut spans,
//!     &mut local_spans,
//! );
//! for event in events {
//!     if let Err(err) = receiver.try_receive(event) {
//!         tracing::warn!(%err, "received invalid tracing event");
//!     }
//! }
//! // Persist the resulting receiver state. There are two pieces
//! // of the state: metadata and alive spans.
//! let mut metadata = PersistedMetadata::default();
//! receiver.persist_metadata(&mut metadata);
//! // `metadata` can be shared among multiple executions of the same executable
//! // (e.g., a WASM module).
//! // `spans` and `local_spans` are specific to the execution; `spans` should
//! // be persisted, while `local_spans` should be stored in RAM.
//! ```

// Documentation settings.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/tracing-tunnel/0.1.0")]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

#[cfg(feature = "receiver")]
#[cfg_attr(docsrs, doc(cfg(feature = "receiver")))]
mod receiver;
#[cfg(feature = "sender")]
#[cfg_attr(docsrs, doc(cfg(feature = "sender")))]
mod sender;
mod types;

#[cfg(feature = "receiver")]
pub use crate::receiver::{
    LocalSpans, PersistedMetadata, PersistedSpans, ReceiveError, TracingEventReceiver,
};
#[cfg(feature = "sender")]
pub use crate::sender::TracingEventSender;
pub use crate::types::{
    CallSiteData, CallSiteKind, DebugObject, MetadataId, RawSpanId, TracedError, TracedValue,
    TracedValues, TracingEvent, TracingLevel, ValueVisitor,
};

#[cfg(doctest)]
doc_comment::doctest!("../README.md");
