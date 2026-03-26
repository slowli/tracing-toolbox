//! Predicates for [`CapturedSpan`]s and [`CapturedEvent`]s.
//!
//! # Overview
//!
//! A predicate can be created with the functions from this module:
//!
//! - [`level()`] checks the span / event level
//! - [`name()`] checks the span name
//! - [`target()`] checks the span / event target
//! - [`field()`] checks a specific span / event field
//! - [`message()`] checks the event message
//! - [`parent()`] checks the direct parent span of an event / span
//! - [`ancestor()`] checks the ancestor spans of an event / span
//!
//! These predicates can be combined with bitwise operators, `&` and `|`.
//! The [`ScanExt`] trait may be used to simplify assertions with predicates. The remaining
//! traits and structs are lower-level plumbing and rarely need to be used directly.
//!
//! [`CapturedSpan`]: crate::CapturedSpan
//! [`CapturedEvent`]: crate::CapturedEvent
//!
//! # Examples
//!
//! ```
//! # use tracing_core::Level;
//! # use predicates::ord::eq;
//! # use tracing_capture::{predicates::*, Storage};
//! # fn test_wrapper(storage: &Storage) {
//! // Predicates can be combined using bitwise operators:
//! let predicate = target([eq("tracing")])
//!     & name(eq("test_capture"))
//!     & level(Level::INFO)
//!     & field("result", 42_i64);
//! // The resulting predicate can be used with `CapturedExt` trait.
//! let storage: &Storage = // ...
//! #   storage;
//! let _ = storage.scan_spans().first(&predicate);
//! let _ = storage.scan_events().single(&level(Level::ERROR));
//!
//! // ...or converted back to a closure:
//! let _ = storage.all_spans().filter(into_fn(predicate));
//! # }
//! ```

use predicates::Predicate;

pub use self::{
    combinators::{And, Or},
    ext::{ScanExt, Scanner},
    field::{
        field, message, value, FieldPredicate, IntoFieldPredicate, MessagePredicate, ValuePredicate,
    },
    level::{level, IntoLevelPredicate, LevelPredicate},
    name::{name, NamePredicate},
    parent::{ancestor, parent, AncestorPredicate, ParentPredicate},
    target::{target, IntoTargetPredicate, TargetPredicate},
};

#[macro_use]
mod combinators;
mod ext;
mod field;
mod level;
mod name;
mod parent;
mod target;
#[cfg(test)]
mod tests;

/// Converts a predicate into an `Fn(_) -> bool` closure.
///
/// This can be used in APIs (e.g., [`Iterator`] combinators) that expect a closure
/// as an argument.
///
/// # Examples
///
/// ```
/// # use tracing_core::Level;
/// # use tracing_capture::{predicates::*, CapturedEvent};
/// let predicate = into_fn(target("tracing") & level(Level::INFO));
/// let events: &[CapturedEvent] = // ...
/// #   &[];
/// let matching_events = events.iter().copied().filter(predicate);
/// // Do something with `matching_events`...
/// ```
pub fn into_fn<Item>(predicate: impl Predicate<Item>) -> impl Fn(&Item) -> bool {
    move |variable| predicate.eval(variable)
}
