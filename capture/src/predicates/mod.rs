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
//! The [`CapturedExt`] trait may be used to simplify assertions with predicates. The remaining
//! traits and structs are lower-level plumbing and rarely need to be used directly.
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
//! let matching_span = storage.spans().find_first(predicate);
//! // ...or converted back to a closure:
//! let predicate = into_fn(predicate);
//! let matching_spans = storage.spans().iter().filter(|&span| predicate(span));
//! // ^ Unfortunately, `filter()` creates a double reference, thus,
//! // `filter(predicate)` doesn't work.
//! # }
//! ```

use predicates::Predicate;

use std::fmt;

#[macro_use]
mod combinators;
mod field;
mod level;
mod name;
mod target;

#[cfg(test)]
mod tests;

pub use self::{
    combinators::{And, Or},
    field::{field, message, FieldPredicate, IntoFieldPredicate, MessagePredicate},
    level::{level, IntoLevelPredicate, LevelPredicate},
    name::{name, NamePredicate},
    target::{target, IntoTargetPredicate, TargetPredicate},
};

use crate::{CapturedEvent, CapturedSpan};

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

/// Extension for assertions on the slices of [`CapturedSpan`]s or [`CapturedEvent`]s.
pub trait CapturedExt {
    /// Slice item.
    type Item;

    /// Finds the first item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items match the predicate.
    fn find_first(&self, predicate: impl Predicate<Self::Item>) -> &Self::Item;

    /// Finds the last item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items match the predicate.
    fn find_last(&self, predicate: impl Predicate<Self::Item>) -> &Self::Item;

    /// Finds the single item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items, or multiple items match the predicate.
    fn find_single(&self, predicate: impl Predicate<Self::Item>) -> &Self::Item;
}

macro_rules! impl_captured_ext {
    ($target:ty) => {
        impl CapturedExt for [$target] {
            type Item = $target;

            fn find_first(&self, predicate: impl Predicate<Self::Item>) -> &Self::Item {
                self.iter()
                    .find(|item| predicate.eval(item))
                    .unwrap_or_else(|| {
                        panic!(
                            "no matches for predicate {predicate} from {snippet:?}",
                            snippet = Snippet(self)
                        )
                    })
            }

            fn find_last(&self, predicate: impl Predicate<Self::Item>) -> &Self::Item {
                self.iter()
                    .rev()
                    .find(|item| predicate.eval(item))
                    .unwrap_or_else(|| {
                        panic!(
                            "no spans matched predicate {predicate} from {snippet:#?}",
                            snippet = Snippet(self)
                        )
                    })
            }

            fn find_single(&self, predicate: impl Predicate<Self::Item>) -> &Self::Item {
                let mut it = self.iter();
                let matched = it.find(|item| predicate.eval(item)).unwrap_or_else(|| {
                    panic!(
                        "no spans matched predicate {predicate} from {snippet:?}",
                        snippet = Snippet(self)
                    )
                });
                if let Some(another_match) = it.find(|item| predicate.eval(item)) {
                    panic!(
                        "multiple matches for predicate {predicate}: {matches:?}",
                        matches = [matched, another_match]
                    );
                }
                matched
            }
        }
    };
}

impl_captured_ext!(CapturedSpan);
impl_captured_ext!(CapturedEvent);

struct Snippet<'a, T>(&'a [T]);

impl<T: fmt::Debug> fmt::Debug for Snippet<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DISPLAYED_ITEMS: usize = 3;

        if self.0.len() <= DISPLAYED_ITEMS {
            write!(formatter, "{:#?}", &self.0)
        } else {
            write!(
                formatter,
                "{:#?} and {} more items",
                &self.0,
                self.0.len() - DISPLAYED_ITEMS
            )
        }
    }
}
