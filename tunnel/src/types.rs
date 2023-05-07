//! Types to carry tracing events over the WASM client-host boundary.

use serde::{Deserialize, Serialize};
use tracing_core::{Level, Metadata};

use core::hash::Hash;
#[cfg(feature = "std")]
use std::path;

use crate::{
    alloc::{Cow, HashMap, String, Vec},
    TracedValues,
};

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

impl TracingEvent {
    /// Normalizes a captured sequence of events so that it does not contain information that
    /// changes between program runs (e.g., metadata IDs) or due to minor refactoring
    /// (source code lines). Normalized events can be used for snapshot testing
    /// and other purposes when reproducibility is important.
    pub fn normalize(events: &mut [Self]) {
        let mut metadata_id_mapping = HashMap::new();
        for event in events {
            match event {
                TracingEvent::NewCallSite { id, data } => {
                    // Replace metadata ID to be predictable.
                    let new_metadata_id = metadata_id_mapping.len() as MetadataId;
                    metadata_id_mapping.insert(*id, new_metadata_id);
                    *id = new_metadata_id;
                    // Normalize file paths to have `/` path delimiters.
                    #[cfg(feature = "std")]
                    if path::MAIN_SEPARATOR != '/' {
                        data.file = data
                            .file
                            .as_deref()
                            .map(|path| path.replace(path::MAIN_SEPARATOR, "/").into());
                    }
                    // Make event data not depend on specific lines, which could easily
                    // change due to refactoring etc.
                    data.line = None;
                    if matches!(data.kind, CallSiteKind::Event) {
                        data.name = Cow::Borrowed("event");
                    }
                }
                TracingEvent::NewSpan { metadata_id, .. }
                | TracingEvent::NewEvent { metadata_id, .. } => {
                    let new_metadata_id = metadata_id_mapping.len() as MetadataId;
                    *metadata_id = *metadata_id_mapping
                        .entry(*metadata_id)
                        .or_insert(new_metadata_id);
                }
                _ => { /* No changes */ }
            }
        }
    }
}
