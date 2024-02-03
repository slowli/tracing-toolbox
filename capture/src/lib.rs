//! Capturing tracing spans and events, e.g. for testing purposes.
//!
//! The core type in this crate is [`CaptureLayer`], a tracing [`Layer`] that can be used
//! to capture tracing spans and events.
//!
//! [`Layer`]: tracing_subscriber::Layer
//!
//! # Examples
//!
//! ```
//! use tracing::Level;
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_capture::{CaptureLayer, SharedStorage};
//!
//! let subscriber = tracing_subscriber::fmt()
//!     .pretty()
//!     .with_max_level(Level::INFO)
//!     .finish();
//! // Add the capturing layer.
//! let storage = SharedStorage::default();
//! let subscriber = subscriber.with(CaptureLayer::new(&storage));
//!
//! // Capture tracing information.
//! tracing::subscriber::with_default(subscriber, || {
//!     tracing::info_span!("test", num = 42_i64).in_scope(|| {
//!         tracing::warn!("I feel disturbance in the Force...");
//!     });
//! });
//!
//! // Inspect the only captured span.
//! let storage = storage.lock();
//! assert_eq!(storage.all_spans().len(), 1);
//! let span = storage.all_spans().next().unwrap();
//! assert_eq!(span["num"], 42_i64);
//! assert_eq!(span.stats().entered, 1);
//! assert!(span.stats().is_closed);
//!
//! // Inspect the only event in the span.
//! let event = span.events().next().unwrap();
//! assert_eq!(*event.metadata().level(), Level::WARN);
//! assert_eq!(
//!     event.message(),
//!     Some("I feel disturbance in the Force...")
//! );
//! ```
//!
//! # Alternatives / similar tools
//!
//! - [`tracing-test`] is a lower-level alternative.
//! - [`tracing-fluent-assertions`] is more similar in its goals, but differs significantly
//!   in the API design; e.g., the assertions need to be declared before the capture.
//!
//! [`tracing-test`]: https://docs.rs/tracing-test
//! [`tracing-fluent-assertions`]: https://docs.rs/tracing-fluent-assertions

// Documentation settings.
#![doc(html_root_url = "https://docs.rs/tracing-capture/0.1.0")]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use tracing_core::Metadata;

use std::{cmp, fmt, ops, ptr};

mod iter;
mod layer;
pub mod predicates;

pub use crate::{
    iter::{CapturedEvents, CapturedSpans, DescendantEvents, DescendantSpans},
    layer::{CaptureLayer, SharedStorage, Storage},
};

use tracing_tunnel::{TracedValue, TracedValues};

mod sealed {
    pub trait Sealed {}
}

#[derive(Debug)]
struct CapturedEventInner {
    metadata: &'static Metadata<'static>,
    values: TracedValues<&'static str>,
    id: CapturedEventId,
    parent_id: Option<CapturedSpanId>,
}

type CapturedEventId = id_arena::Id<CapturedEventInner>;

/// Captured tracing event containing a reference to its [`Metadata`] and values that the event
/// was created with.
///
/// `CapturedEvent`s are comparable and are [partially ordered](PartialOrd) according
/// to the capture order. Events are considered equal iff both are aliases of the same event;
/// i.e., equality is reference-based rather than content-based.
/// Two events from different [`Storage`]s are not ordered and are always non-equal.
///
/// Values recorded with the event can be accessed by indexing or using [`Self::value()`],
/// or iterated over using [`Self::values()`].
///
/// # Examples
///
/// ```
/// # use tracing_core::Level;
/// # use tracing_capture::CapturedEvent;
/// # fn test_wrapper(event: CapturedEvent) {
/// let event: CapturedEvent = // ...
/// #   event;
/// // Accessing event metadata and fields:
/// assert_eq!(*event.metadata().level(), Level::INFO);
/// assert_eq!(event["return"], 42_u64);
/// assert_eq!(event.message(), Some("finished computations"));
/// assert!(event.value("err").is_none());
/// // Filtering unsigned integer values:
/// let numbers = event.values().filter_map(|(_, val)| val.as_uint());
///
/// // Accessing the parent span:
/// let parent_name = event.parent().unwrap().metadata().name();
/// assert!(event
///     .ancestors()
///     .any(|span| span.metadata().name() == "test"));
/// # }
/// ```
#[derive(Clone, Copy)]
pub struct CapturedEvent<'a> {
    inner: &'a CapturedEventInner,
    storage: &'a Storage,
}

impl fmt::Debug for CapturedEvent<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, formatter)
    }
}

impl<'a> CapturedEvent<'a> {
    /// Provides a reference to the event metadata.
    pub fn metadata(&self) -> &'static Metadata<'static> {
        self.inner.metadata
    }

    /// Iterates over values associated with the event.
    pub fn values(&self) -> impl Iterator<Item = (&'a str, &'a TracedValue)> + 'a {
        self.inner.values.iter()
    }

    /// Returns a value for the specified field, or `None` if the value is not defined.
    pub fn value(&self, name: &str) -> Option<&'a TracedValue> {
        self.inner.values.get(name)
    }

    /// Returns the message recorded in this event, i.e., the value of the `message` field
    /// if it has a string presentation.
    pub fn message(&self) -> Option<&'a str> {
        self.value("message").and_then(|message| match message {
            TracedValue::Object(obj) => Some(obj.as_ref()),
            TracedValue::String(s) => Some(s),
            TracedValue::Error(err) => Some(&err.message),
            _ => None,
        })
    }

    /// Returns the parent span for this event, or `None` if is not tied to a captured span.
    pub fn parent(&self) -> Option<CapturedSpan<'a>> {
        self.inner.parent_id.map(|id| self.storage.span(id))
    }

    /// Returns the references to the ancestor spans, starting from the direct parent
    /// and ending in one of [root spans](Storage::root_spans()).
    pub fn ancestors(&self) -> impl Iterator<Item = CapturedSpan<'a>> + '_ {
        std::iter::successors(self.parent(), CapturedSpan::parent)
    }
}

impl PartialEq for CapturedEvent<'_> {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.storage, other.storage) && self.inner.id == other.inner.id
    }
}

impl Eq for CapturedEvent<'_> {}

impl PartialOrd for CapturedEvent<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        if ptr::eq(self.storage, other.storage) {
            Some(self.inner.id.cmp(&other.inner.id))
        } else {
            None
        }
    }
}

impl ops::Index<&str> for CapturedEvent<'_> {
    type Output = TracedValue;

    fn index(&self, index: &str) -> &Self::Output {
        self.value(index)
            .unwrap_or_else(|| panic!("field `{index}` is not contained in event"))
    }
}

/// Statistics about a [`CapturedSpan`].
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct SpanStats {
    /// Number of times the span was entered.
    pub entered: usize,
    /// Number of times the span was exited.
    pub exited: usize,
    /// Is the span closed (dropped)?
    pub is_closed: bool,
}

#[derive(Debug)]
struct CapturedSpanInner {
    metadata: &'static Metadata<'static>,
    values: TracedValues<&'static str>,
    stats: SpanStats,
    id: CapturedSpanId,
    parent_id: Option<CapturedSpanId>,
    child_ids: Vec<CapturedSpanId>,
    event_ids: Vec<CapturedEventId>,
    follows_from_ids: Vec<CapturedSpanId>,
}

type CapturedSpanId = id_arena::Id<CapturedSpanInner>;

/// Captured tracing span containing a reference to its [`Metadata`], values that the span
/// was created with, [stats](SpanStats), and descendant [`CapturedEvent`]s.
///
/// `CapturedSpan`s are comparable and are [partially ordered](PartialOrd) according
/// to the capture order. Spans are considered equal iff both are aliases of the same span;
/// i.e., equality is reference-based rather than content-based.
/// Two spans from different [`Storage`]s are not ordered and are always non-equal.
///
/// Values recorded with the span can be accessed by indexing or using [`Self::value()`],
/// or iterated over using [`Self::values()`].
///
/// # Examples
///
/// ```
/// # use tracing_core::Level;
/// # use tracing_capture::CapturedSpan;
/// # fn test_wrapper(span: CapturedSpan) {
/// let span: CapturedSpan = // ...
/// #   span;
/// // Accessing event metadata and fields:
/// assert_eq!(*span.metadata().level(), Level::INFO);
/// assert_eq!(span["arg"], 42_u64);
/// assert!(span.value("other_arg").is_none());
/// // Filtering unsigned integer values:
/// let numbers = span.values().filter_map(|(_, val)| val.as_uint());
///
/// // Accessing the parent span:
/// let parent_name = span.parent().unwrap().metadata().name();
/// assert!(span
///     .ancestors()
///     .any(|span| span.metadata().name() == "test"));
///
/// // Accessing child spans and events:
/// assert!(span.children().len() > 0);
/// let child_messages: Vec<&str> = span
///     .events()
///     .filter_map(|event| event.message())
///     .collect();
/// let descendant_span =
///     span.descendants().find(|span| span["input"] == "!").unwrap();
/// # }
/// ```
#[derive(Clone, Copy)]
pub struct CapturedSpan<'a> {
    inner: &'a CapturedSpanInner,
    storage: &'a Storage,
}

impl fmt::Debug for CapturedSpan<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, formatter)
    }
}

impl<'a> CapturedSpan<'a> {
    /// Provides a reference to the span metadata.
    pub fn metadata(&self) -> &'static Metadata<'static> {
        self.inner.metadata
    }

    /// Iterates over values that the span was created with, or which were recorded later.
    pub fn values(&self) -> impl Iterator<Item = (&'a str, &'a TracedValue)> + 'a {
        self.inner.values.iter()
    }

    /// Returns a value for the specified field, or `None` if the value is not defined.
    pub fn value(&self, name: &str) -> Option<&'a TracedValue> {
        self.inner.values.get(name)
    }

    /// Returns statistics about span operations.
    pub fn stats(&self) -> SpanStats {
        self.inner.stats
    }

    /// Returns events attached to this span.
    pub fn events(&self) -> CapturedEvents<'a> {
        CapturedEvents::from_slice(self.storage, &self.inner.event_ids)
    }

    /// Returns the reference to the parent span, if any.
    pub fn parent(&self) -> Option<Self> {
        self.inner.parent_id.map(|id| self.storage.span(id))
    }

    /// Returns the references to the ancestor spans, starting from the direct parent
    /// and ending in one of [root spans](Storage::root_spans()).
    pub fn ancestors(&self) -> impl Iterator<Item = CapturedSpan<'a>> + '_ {
        std::iter::successors(self.parent(), Self::parent)
    }

    /// Iterates over the direct children of this span, in the order of their capture.
    pub fn children(&self) -> CapturedSpans<'a> {
        CapturedSpans::from_slice(self.storage, &self.inner.child_ids)
    }

    /// Iterates over the descendants of this span.
    ///
    /// In the simplest case (spans are not re-entered, span parents are contextual), the iteration
    /// order is the span capture order. In the general case, no particular order is guaranteed.
    pub fn descendants(&self) -> DescendantSpans<'a> {
        DescendantSpans::new(self)
    }

    /// Iterates over the [events](CapturedEvent) of the [descendants](Self::descendants())
    /// of this span. The iteration order is not specified. The returned events do not include
    /// the events [directly attached](Self::events()) to this span; if you need them to be included,
    /// use something like `span.events().chain(span.descendant_events())`.
    pub fn descendant_events(&self) -> DescendantEvents<'a> {
        DescendantEvents::new(self)
    }

    /// Iterates over the spans this span follows from.
    pub fn follows_from(&self) -> CapturedSpans<'a> {
        CapturedSpans::from_slice(self.storage, &self.inner.follows_from_ids)
    }
}

impl PartialEq for CapturedSpan<'_> {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.storage, other.storage) && self.inner.id == other.inner.id
    }
}

impl Eq for CapturedSpan<'_> {}

impl PartialOrd for CapturedSpan<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        if ptr::eq(self.storage, other.storage) {
            Some(self.inner.id.cmp(&other.inner.id))
        } else {
            None
        }
    }
}

impl ops::Index<&str> for CapturedSpan<'_> {
    type Output = TracedValue;

    fn index(&self, index: &str) -> &Self::Output {
        self.value(index)
            .unwrap_or_else(|| panic!("field `{index}` is not contained in span"))
    }
}

/// Uniting trait for [`CapturedSpan`]s and [`CapturedEvent`]s that allows writing generic
/// code in cases both should be supported.
pub trait Captured<'a>: Eq + PartialOrd + sealed::Sealed {
    /// Provides a reference to the span / event metadata.
    fn metadata(&self) -> &'static Metadata<'static>;
    /// Returns a value for the specified field, or `None` if the value is not defined.
    fn value(&self, name: &str) -> Option<&'a TracedValue>;
    /// Returns the reference to the parent span, if any.
    fn parent(&self) -> Option<CapturedSpan<'a>>;
}

impl sealed::Sealed for CapturedSpan<'_> {}

impl<'a> Captured<'a> for CapturedSpan<'a> {
    #[inline]
    fn metadata(&self) -> &'static Metadata<'static> {
        self.metadata()
    }

    #[inline]
    fn value(&self, name: &str) -> Option<&'a TracedValue> {
        self.value(name)
    }

    #[inline]
    fn parent(&self) -> Option<CapturedSpan<'a>> {
        self.parent()
    }
}

impl sealed::Sealed for CapturedEvent<'_> {}

impl<'a> Captured<'a> for CapturedEvent<'a> {
    #[inline]
    fn metadata(&self) -> &'static Metadata<'static> {
        self.metadata()
    }

    #[inline]
    fn value(&self, name: &str) -> Option<&'a TracedValue> {
        self.value(name)
    }

    #[inline]
    fn parent(&self) -> Option<CapturedSpan<'a>> {
        self.parent()
    }
}

#[cfg(doctest)]
doc_comment::doctest!("../README.md");
