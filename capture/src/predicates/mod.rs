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
//!
//! These predicates can be combined with bitwise operators, `&` and `|`.
//! The [`ScannerExt`] trait may be used to simplify assertions with predicates. The remaining
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
//! let _ = storage.all_spans().scanner().first(&predicate);
//! let _ = storage.all_events().scanner().single(&level(Level::ERROR));
//!
//! // ...or converted back to a closure:
//! let predicate = into_fn(predicate);
//! let _ = storage.all_spans().iter().filter(|&span| predicate(span));
//! // ^ Unfortunately, `filter()` creates a double reference, thus,
//! // `filter(predicate)` doesn't work.
//! # }
//! ```

use predicates::Predicate;

#[macro_use]
mod combinators;
mod ext;
mod field;
mod level;
mod name;
mod target;

#[cfg(test)]
mod tests;

pub use self::{
    combinators::{And, Or},
    ext::{Scanner, ScannerExt},
    field::{field, message, FieldPredicate, IntoFieldPredicate, MessagePredicate},
    level::{level, IntoLevelPredicate, LevelPredicate},
    name::{name, NamePredicate},
    target::{target, IntoTargetPredicate, TargetPredicate},
};

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
/// let matching_events = events.iter().filter(|&evt| predicate(evt));
/// // Do something with `matching_events`...
/// ```
pub fn into_fn<Item>(predicate: impl Predicate<Item>) -> impl Fn(&Item) -> bool {
    move |variable| predicate.eval(variable)
}
