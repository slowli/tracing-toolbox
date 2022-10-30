//! `field()` and `message()` predicate factories.

use predicates::{
    reflection::{Case, PredicateReflection, Product},
    Predicate,
};

use std::fmt;

use crate::{CapturedEvent, CapturedSpan};
use tracing_tunnel::TracedValue;

/// Conversion into a predicate for a [`TracedValue`] used in the [`field()`] function.
pub trait IntoFieldPredicate {
    /// Predicate output of the conversion. The exact type should be considered an implementation
    /// detail and should not be relied upon.
    type Predicate: Predicate<TracedValue>;
    /// Performs the conversion.
    fn into_predicate(self) -> Self::Predicate;
}

impl<P: Predicate<TracedValue>> IntoFieldPredicate for [P; 1] {
    type Predicate = P;

    fn into_predicate(self) -> Self::Predicate {
        self.into_iter().next().unwrap()
    }
}

macro_rules! impl_into_field_predicate {
    ($($ty:ty),+) => {
        $(
        impl IntoFieldPredicate for $ty {
            type Predicate = EquivPredicate<Self>;

            fn into_predicate(self) -> Self::Predicate {
                EquivPredicate { value: self }
            }
        }
        )+
    };
}

impl_into_field_predicate!(bool, i64, i128, u64, u128, f64, &str);

/// Creates a predicate for a particular field of a [`CapturedSpan`] or [`CapturedEvent`].
///
/// # Arguments
///
/// The argument of this function is essentially a predicate for the [`TracedValue`] of the field.
/// It may be:
///
/// - `bool`, `i64`, `i128`, `u64`, `u128`, `f64`, `&str`: will be compared to the `TracedValue`
///   using the corresponding [`PartialEq`] implementation.
/// - Any `Predicate` for [`TracedValue`]. To bypass Rust orphaning rules, the predicate
///   must be enclosed in square brackets (i.e., a one-value array).
///
/// # Examples
///
/// ```
/// # use predicates::constant::always;
/// # use tracing_subscriber::{layer::SubscriberExt, Registry};
/// # use tracing_capture::{predicates::{field, ScannerExt}, CaptureLayer, SharedStorage};
/// let storage = SharedStorage::default();
/// let subscriber = Registry::default().with(CaptureLayer::new(&storage));
/// tracing::subscriber::with_default(subscriber, || {
///     tracing::info_span!("compute", arg = 5_i32).in_scope(|| {
///         tracing::info!("done");
///     });
/// });
///
/// let storage = storage.lock();
/// // All of these access the single captured span.
/// let spans = storage.spans().scanner();
/// let _ = spans.single(&field("arg", [always()]));
/// let _ = spans.single(&field("arg", 5_i64));
/// ```
pub fn field<P: IntoFieldPredicate>(
    name: &'static str,
    matches: P,
) -> FieldPredicate<P::Predicate> {
    FieldPredicate {
        name,
        matches: matches.into_predicate(),
    }
}

/// Predicate for a particular field of a [`CapturedSpan`] or [`CapturedEvent`] returned by
/// the [`field()`] function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldPredicate<P> {
    name: &'static str,
    matches: P,
}

impl_bool_ops!(FieldPredicate<P>);

impl<P: Predicate<TracedValue>> fmt::Display for FieldPredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "fields.{}({})", self.name, self.matches)
    }
}

impl<P: Predicate<TracedValue>> PredicateReflection for FieldPredicate<P> {}

macro_rules! impl_predicate_for_field {
    ($ty:ty) => {
        impl<P: Predicate<TracedValue>> Predicate<$ty> for FieldPredicate<P> {
            fn eval(&self, variable: &$ty) -> bool {
                variable
                    .value(self.name)
                    .map_or(false, |value| self.matches.eval(value))
            }

            fn find_case(&self, expected: bool, variable: &$ty) -> Option<Case<'_>> {
                let value = if let Some(value) = variable.value(self.name) {
                    value
                } else {
                    return if expected {
                        None // was expecting a variable, but there is none
                    } else {
                        let product = Product::new(format!("fields.{}", self.name), "None");
                        Some(Case::new(Some(self), expected).add_product(product))
                    };
                };

                let child = self.matches.find_case(expected, value)?;
                Some(Case::new(Some(self), expected).add_child(child))
            }
        }
    };
}

impl_predicate_for_field!(CapturedSpan);
impl_predicate_for_field!(CapturedEvent);

#[doc(hidden)] // implementation detail (yet?)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquivPredicate<V> {
    value: V,
}

impl<V: fmt::Debug> fmt::Display for EquivPredicate<V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "var == {:?}", self.value)
    }
}

impl<V: fmt::Debug> PredicateReflection for EquivPredicate<V> {}

impl<V: fmt::Debug + PartialEq<TracedValue>> Predicate<TracedValue> for EquivPredicate<V> {
    fn eval(&self, variable: &TracedValue) -> bool {
        self.value == *variable
    }

    fn find_case(&self, expected: bool, variable: &TracedValue) -> Option<Case<'_>> {
        if self.eval(variable) == expected {
            let product = Product::new("var", format!("{variable:?}"));
            Some(Case::new(Some(self), expected).add_product(product))
        } else {
            None
        }
    }
}

/// Creates a predicate for the message of a [`CapturedEvent`].
///
/// # Arguments
///
/// The argument of this function is a `str` predicate for the event message.
///
/// # Examples
///
/// ```
/// # use predicates::{ord::eq, str::contains};
/// # use tracing_subscriber::{layer::SubscriberExt, Registry};
/// # use tracing_capture::{predicates::{message, ScannerExt}, CaptureLayer, SharedStorage};
/// let storage = SharedStorage::default();
/// let subscriber = Registry::default().with(CaptureLayer::new(&storage));
/// tracing::subscriber::with_default(subscriber, || {
///     tracing::info_span!("compute").in_scope(|| {
///         tracing::info!(result = 42, "computations completed");
///     });
/// });
///
/// let storage = storage.lock();
/// // All of these access the single captured event.
/// let events = storage.all_events().scanner();
/// let _ = events.single(&message(eq("computations completed")));
/// let events = storage.all_events().scanner();
/// let _ = events.single(&message(contains("completed")));
/// ```
pub fn message<P: Predicate<str>>(matches: P) -> MessagePredicate<P> {
    MessagePredicate { matches }
}

/// Predicate for the message of a [`CapturedEvent`] returned by the [`message()`] function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagePredicate<P> {
    matches: P,
}

impl_bool_ops!(MessagePredicate<P>);

impl<P: Predicate<str>> fmt::Display for MessagePredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "message({})", self.matches)
    }
}

impl<P: Predicate<str>> PredicateReflection for MessagePredicate<P> {}

impl<P: Predicate<str>> Predicate<CapturedEvent> for MessagePredicate<P> {
    fn eval(&self, variable: &CapturedEvent) -> bool {
        let message = variable.value("message").and_then(str_presentation);
        message.map_or(false, |value| self.matches.eval(value))
    }

    fn find_case(&self, expected: bool, variable: &CapturedEvent) -> Option<Case<'_>> {
        let message = variable.value("message").and_then(str_presentation);
        let message = if let Some(message) = message {
            message
        } else {
            return if expected {
                None // was expecting a variable, but there is none
            } else {
                let product = Product::new("message", "None");
                Some(Case::new(Some(self), expected).add_product(product))
            };
        };

        let child = self.matches.find_case(expected, message)?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

fn str_presentation(value: &TracedValue) -> Option<&str> {
    match value {
        TracedValue::String(str) => Some(str),
        TracedValue::Object(object) => Some(object.as_ref()),
        _ => None,
    }
}
