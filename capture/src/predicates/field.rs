//! `field()` and `message()` predicate factories.

use predicates::{
    reflection::{Case, PredicateReflection, Product},
    Predicate,
};

use std::{any::type_name, borrow::Borrow, fmt, marker::PhantomData};

use crate::{Captured, CapturedEvent};
use tracing_tunnel::{FromTracedValue, TracedValue};

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
/// - A predicate produced by the [`value()`] function.
/// - Any `Predicate` for [`TracedValue`]. To bypass Rust orphaning rules, the predicate
///   must be enclosed in square brackets (i.e., a one-value array).
///
/// [`CapturedSpan`]: crate::CapturedSpan
///
/// # Examples
///
/// ```
/// # use predicates::{constant::always, ord::gt};
/// # use tracing_subscriber::{layer::SubscriberExt, Registry};
/// # use tracing_capture::{predicates::{field, value, ScanExt}, CaptureLayer, SharedStorage};
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
/// let spans = storage.scan_spans();
/// let _ = spans.single(&field("arg", [always()]));
/// let _ = spans.single(&field("arg", 5_i64));
/// let _ = spans.single(&field("arg", value(gt(3_i64))));
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
///
/// [`CapturedSpan`]: crate::CapturedSpan
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

impl<'a, P: Predicate<TracedValue>, T: Captured<'a>> Predicate<T> for FieldPredicate<P> {
    fn eval(&self, variable: &T) -> bool {
        variable
            .value(self.name)
            .map_or(false, |value| self.matches.eval(value))
    }

    fn find_case(&self, expected: bool, variable: &T) -> Option<Case<'_>> {
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

/// Creates a predicate for a [`TracedValue`] that checks whether the value matches
/// the specified criteria for a particular subtype (e.g., an unsigned integer).
/// If the value has another subtype, the predicate is false.
///
/// Returned predicates can be supplied to the [`field()`] function.
///
/// # Arguments
///
/// The argument must be a predicate for one of types that can be obtained from a [`TracedValue`]
/// reference: `bool`, `i64`, `i128`, `u64`, `u128`, `f64`, or `str`. The type can be specified
/// explicitly, but usually you can make Rust properly infer it.
///
/// # Examples
///
/// ```
/// # use predicates::{ord::{gt, ne}, iter::in_hash, str::contains};
/// # use tracing_capture::predicates::{field, value};
/// let _ = field("return", value(gt(5.0)));
/// let _ = field("name", value(contains("test")));
/// let _ = field("float", value(in_hash([3_u64, 5])));
/// // ^ Note the specified integer type.
/// ```
pub fn value<T, P>(matches: P) -> ValuePredicate<T, P>
where
    T: for<'a> FromTracedValue<'a> + ?Sized,
    P: Predicate<T>,
{
    ValuePredicate {
        matches,
        _ty: PhantomData,
    }
}

/// Predicate for [`TracedValue`]s returned by the [`value()`] function.
#[derive(Debug)]
pub struct ValuePredicate<T: ?Sized, P> {
    matches: P,
    _ty: PhantomData<fn(T)>,
}

impl<T: ?Sized, P: Clone> Clone for ValuePredicate<T, P> {
    fn clone(&self) -> Self {
        Self {
            matches: self.matches.clone(),
            _ty: PhantomData,
        }
    }
}

impl<T: ?Sized, P: Copy> Copy for ValuePredicate<T, P> {}

impl<T: ?Sized, P: PartialEq> PartialEq for ValuePredicate<T, P> {
    fn eq(&self, other: &Self) -> bool {
        self.matches == other.matches
    }
}

impl<T, P> fmt::Display for ValuePredicate<T, P>
where
    T: for<'a> FromTracedValue<'a> + ?Sized,
    P: Predicate<T>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "is<{}>({})", type_name::<T>(), self.matches)
    }
}

impl<T, P> PredicateReflection for ValuePredicate<T, P>
where
    T: for<'a> FromTracedValue<'a> + ?Sized,
    P: Predicate<T>,
{
}

impl<T, P> Predicate<TracedValue> for ValuePredicate<T, P>
where
    T: for<'a> FromTracedValue<'a> + ?Sized,
    P: Predicate<T>,
{
    fn eval(&self, variable: &TracedValue) -> bool {
        T::from_value(variable).map_or(false, |value| self.matches.eval(value.borrow()))
    }

    fn find_case(&self, expected: bool, variable: &TracedValue) -> Option<Case<'_>> {
        let value = T::from_value(variable);
        let value = if let Some(value) = &value {
            value.borrow()
        } else {
            return if expected {
                None // was expecting another var type
            } else {
                let product = Product::new(format!("var.as<{}>", type_name::<T>()), "None");
                Some(Case::new(Some(self), expected).add_product(product))
            };
        };

        let child = self.matches.find_case(expected, value)?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

impl<T, P> IntoFieldPredicate for ValuePredicate<T, P>
where
    T: for<'a> FromTracedValue<'a> + ?Sized,
    P: Predicate<T>,
{
    type Predicate = Self;

    fn into_predicate(self) -> Self::Predicate {
        self
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
/// # use tracing_capture::{predicates::{message, ScanExt}, CaptureLayer, SharedStorage};
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
/// let events = storage.scan_events();
/// let _ = events.single(&message(eq("computations completed")));
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

impl<P: Predicate<str>> Predicate<CapturedEvent<'_>> for MessagePredicate<P> {
    fn eval(&self, variable: &CapturedEvent<'_>) -> bool {
        variable
            .message()
            .map_or(false, |value| self.matches.eval(value))
    }

    fn find_case(&self, expected: bool, variable: &CapturedEvent<'_>) -> Option<Case<'_>> {
        let message = if let Some(message) = variable.message() {
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
