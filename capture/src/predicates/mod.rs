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

use predicates::{
    reflection::{Case, PredicateReflection, Product},
    Predicate,
};
use tracing_core::{Level, LevelFilter};

use std::fmt;

#[macro_use]
mod combinators;

pub use self::combinators::{And, Or};

use crate::{CapturedEvent, CapturedSpan};
use tracing_tunnel::TracedValue;

/// Conversion into a predicate for [`Level`]s used in the [`level()`] function.
pub trait IntoLevelPredicate {
    /// Predicate output of the conversion. The exact type should be considered an implementation
    /// detail and should not be relied upon.
    type Predicate: Predicate<Level>;
    /// Performs the conversion.
    fn into_predicate(self) -> Self::Predicate;
}

impl<P: Predicate<Level>> IntoLevelPredicate for [P; 1] {
    type Predicate = P;

    fn into_predicate(self) -> Self::Predicate {
        self.into_iter().next().unwrap()
    }
}

impl IntoLevelPredicate for Level {
    type Predicate = predicates::ord::EqPredicate<Level>;

    fn into_predicate(self) -> Self::Predicate {
        predicates::ord::eq(self)
    }
}

impl IntoLevelPredicate for LevelFilter {
    type Predicate = predicates::ord::OrdPredicate<Level>;

    fn into_predicate(self) -> Self::Predicate {
        self.into_level()
            .map_or_else(|| predicates::ord::lt(Level::ERROR), predicates::ord::le)
    }
}

/// Creates a predicate for the [`Level`] of a [`CapturedSpan`] or [`CapturedEvent`].
///
/// # Arguments
///
/// The argument of this function may be:
///
/// - [`Level`]: will be compared exactly
/// - [`LevelFilter`]: will be compared as per ordinary rules
/// - Any `Predicate` for [`Level`]. To bypass Rust orphaning rules, the predicate
///   must be enclosed in square brackets (i.e., a one-value array).
///
/// # Examples
///
/// ```
/// # use predicates::ord::gt;
/// # use tracing_core::{Level, LevelFilter};
/// # use tracing_subscriber::{layer::SubscriberExt, Registry};
/// # use tracing_capture::{predicates::{level, CapturedExt}, CaptureLayer, SharedStorage};
/// let storage = SharedStorage::default();
/// let subscriber = Registry::default().with(CaptureLayer::new(&storage));
/// tracing::subscriber::with_default(subscriber, || {
///     tracing::info_span!("compute").in_scope(|| {
///         tracing::info!(answer = 42, "done");
///     });
/// });
///
/// let storage = storage.lock();
/// // All of these access the single captured span.
/// let span = storage.spans().find_single(level(Level::INFO));
/// let span = storage.spans().find_single(level(LevelFilter::DEBUG));
/// let span = storage.spans().find_single(level([gt(Level::WARN)]));
/// ```
pub fn level<P: IntoLevelPredicate>(matches: P) -> LevelPredicate<P::Predicate> {
    LevelPredicate {
        matches: matches.into_predicate(),
    }
}

/// Predicate for the [`Level`] of a [`CapturedSpan`] or [`CapturedEvent`] returned by
/// the [`level()`] function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelPredicate<P> {
    matches: P,
}

impl<P: Predicate<Level>> fmt::Display for LevelPredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "level({})", self.matches)
    }
}

impl<P: Predicate<Level>> PredicateReflection for LevelPredicate<P> {}

impl<P: Predicate<Level>> Predicate<CapturedSpan> for LevelPredicate<P> {
    fn eval(&self, variable: &CapturedSpan) -> bool {
        self.matches.eval(variable.metadata().level())
    }

    fn find_case(&self, expected: bool, variable: &CapturedSpan) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().level())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

impl<P: Predicate<Level>> Predicate<CapturedEvent> for LevelPredicate<P> {
    fn eval(&self, variable: &CapturedEvent) -> bool {
        self.matches.eval(variable.metadata().level())
    }

    fn find_case(&self, expected: bool, variable: &CapturedEvent) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().level())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

/// Creates a predicate for the name of a [`CapturedSpan`].
///
/// # Arguments
///
/// The argument of this function can be any `str`ing predicate, e.g. `eq("test")` for
/// exact comparison.
///
/// # Examples
///
/// ```
/// # use predicates::{ord::eq, str::starts_with};
/// # use tracing_subscriber::{layer::SubscriberExt, Registry};
/// # use tracing_capture::{predicates::{name, CapturedExt}, CaptureLayer, SharedStorage};
/// let storage = SharedStorage::default();
/// let subscriber = Registry::default().with(CaptureLayer::new(&storage));
/// tracing::subscriber::with_default(subscriber, || {
///     tracing::info_span!("compute").in_scope(|| {
///         tracing::info!(answer = 42, "done");
///     });
/// });
///
/// let storage = storage.lock();
/// // All of these access the single captured span.
/// let span = storage.spans().find_single(name(eq("compute")));
/// let span = storage.spans().find_single(name(starts_with("co")));
/// ```
pub fn name<P: Predicate<str>>(matches: P) -> NamePredicate<P> {
    NamePredicate { matches }
}

/// Predicate for the name of a [`CapturedSpan`] or [`CapturedEvent`] returned by
/// the [`name()`] function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamePredicate<P> {
    matches: P,
}

impl<P: Predicate<str>> fmt::Display for NamePredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "name({})", self.matches)
    }
}

impl<P: Predicate<str>> PredicateReflection for NamePredicate<P> {}

impl<P: Predicate<str>> Predicate<CapturedSpan> for NamePredicate<P> {
    fn eval(&self, variable: &CapturedSpan) -> bool {
        self.matches.eval(variable.metadata().name())
    }

    fn find_case(&self, expected: bool, variable: &CapturedSpan) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().name())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

/// Conversion into a predicate for the target used in the [`target()`] function.
pub trait IntoTargetPredicate {
    /// Predicate output of the conversion. The exact type should be considered an implementation
    /// detail and should not be relied upon.
    type Predicate: Predicate<str>;
    /// Performs the conversion.
    fn into_predicate(self) -> Self::Predicate;
}

impl<P: Predicate<str>> IntoTargetPredicate for [P; 1] {
    type Predicate = P;

    fn into_predicate(self) -> Self::Predicate {
        self.into_iter().next().unwrap()
    }
}

impl IntoTargetPredicate for &str {
    type Predicate = predicates::str::RegexPredicate;

    fn into_predicate(self) -> Self::Predicate {
        predicates::str::is_match(format!("^{self}($|::)")).unwrap()
    }
}

/// Creates a predicate for the target of a [`CapturedSpan`] or [`CapturedEvent`].
///
/// # Arguments
///
/// The argument of this function may be:
///
/// - `&str`: will be compared as per standard target filtering. E.g., `target("tracing")`
///   will match `tracing` and `tracing::predicate` targets, but not `tracing_capture`.
/// - Any `str` `Predicate`. To bypass Rust orphaning rules, the predicate
///   must be enclosed in square brackets (i.e., a one-value array).
///
/// # Examples
///
/// ```
/// # use predicates::str::starts_with;
/// # use tracing_subscriber::{layer::SubscriberExt, Registry};
/// # use tracing_capture::{predicates::{target, CapturedExt}, CaptureLayer, SharedStorage};
/// let storage = SharedStorage::default();
/// let subscriber = Registry::default().with(CaptureLayer::new(&storage));
/// tracing::subscriber::with_default(subscriber, || {
///     tracing::info_span!(target: "capture::test", "compute").in_scope(|| {
///         tracing::info!(answer = 42, "done");
///     });
/// });
///
/// let storage = storage.lock();
/// // All of these access the single captured span.
/// let span = storage.spans().find_single(target("capture"));
/// let span = storage.spans().find_single(target([starts_with("cap")]));
/// ```
pub fn target<P: IntoTargetPredicate>(matches: P) -> TargetPredicate<P::Predicate> {
    TargetPredicate {
        matches: matches.into_predicate(),
    }
}

/// Predicate for the target of a [`CapturedSpan`] or [`CapturedEvent`] returned by
/// the [`target()`] function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPredicate<P> {
    matches: P,
}

impl<P: Predicate<str>> fmt::Display for TargetPredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "target({})", self.matches)
    }
}

impl<P: Predicate<str>> PredicateReflection for TargetPredicate<P> {}

impl<P: Predicate<str>> Predicate<CapturedSpan> for TargetPredicate<P> {
    fn eval(&self, variable: &CapturedSpan) -> bool {
        self.matches.eval(variable.metadata().target())
    }

    fn find_case(&self, expected: bool, variable: &CapturedSpan) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().target())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

impl<P: Predicate<str>> Predicate<CapturedEvent> for TargetPredicate<P> {
    fn eval(&self, variable: &CapturedEvent) -> bool {
        self.matches.eval(variable.metadata().target())
    }

    fn find_case(&self, expected: bool, variable: &CapturedEvent) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().target())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}

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
                equiv(self)
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
/// # use tracing_capture::{predicates::{field, CapturedExt}, CaptureLayer, SharedStorage};
/// let storage = SharedStorage::default();
/// let subscriber = Registry::default().with(CaptureLayer::new(&storage));
/// tracing::subscriber::with_default(subscriber, || {
///     tracing::info_span!("compute", arg = 5_i32).in_scope(|| {
///         tracing::info!("done");
///     });
/// });
///
/// let storage = storage.lock();
/// // All of these access the single captured event.
/// let event = storage.spans().find_single(field("arg", [always()]));
/// let event = storage.spans().find_single(field("arg", 5_i64));
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

fn equiv<V: PartialEq<TracedValue>>(value: V) -> EquivPredicate<V> {
    EquivPredicate { value }
}

#[doc(hidden)] // implementation detail (yet?)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquivPredicate<V> {
    value: V,
}

impl<V: fmt::Debug> fmt::Display for EquivPredicate<V> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "var ~= {:?}", self.value)
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

impl_bool_ops!(TargetPredicate<P>);
impl_bool_ops!(NamePredicate<P>);
impl_bool_ops!(LevelPredicate<P>);
impl_bool_ops!(FieldPredicate<P>);

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

#[cfg(test)]
mod tests {
    use predicates::{
        ord::eq,
        prelude::*,
        str::{ends_with, starts_with},
    };
    use tracing_core::{callsite::DefaultCallsite, field::FieldSet, Kind, Metadata};

    use super::*;
    use crate::SpanStats;
    use tracing_tunnel::TracedValues;

    static SITE: DefaultCallsite = DefaultCallsite::new(METADATA);
    static METADATA: &Metadata<'static> = &Metadata::new(
        "test_span",
        "tracing_capture::predicate",
        Level::INFO,
        Some("predicate.rs"),
        Some(42),
        Some("predicate"),
        FieldSet::new(&["val"], tracing_core::identify_callsite!(&SITE)),
        Kind::SPAN,
    );

    #[test]
    fn level_predicates() {
        let span = CapturedSpan {
            metadata: METADATA,
            values: TracedValues::new(),
            stats: SpanStats::default(),
            events: vec![],
        };

        let predicate = level(Level::INFO);
        assert!(predicate.eval(&span));
        let predicate = level(Level::DEBUG);
        assert!(!predicate.eval(&span));
        let predicate = level(Level::WARN);
        assert!(!predicate.eval(&span));

        let predicate = level(LevelFilter::INFO);
        assert!(predicate.eval(&span));
        let predicate = level(LevelFilter::DEBUG);
        assert!(predicate.eval(&span));
        let predicate = level(LevelFilter::WARN);
        assert!(!predicate.eval(&span));
        let predicate = level(LevelFilter::OFF);
        assert!(!predicate.eval(&span));
    }

    #[test]
    fn target_predicates() {
        let span = CapturedSpan {
            metadata: METADATA,
            values: TracedValues::new(),
            stats: SpanStats::default(),
            events: vec![],
        };

        let predicate = target("tracing_capture");
        assert!(predicate.eval(&span));
        let predicate = target("tracing");
        assert!(!predicate.eval(&span));
        let predicate = target("tracing_capture::predicate");
        assert!(predicate.eval(&span));
        let predicate = target("tracing_capture::pred");
        assert!(!predicate.eval(&span));
    }

    #[test]
    fn name_predicates() {
        let span = CapturedSpan {
            metadata: METADATA,
            values: TracedValues::new(),
            stats: SpanStats::default(),
            events: vec![],
        };

        let predicate = name(eq("test_span"));
        assert!(predicate.eval(&span));
        let predicate = name(starts_with("test"));
        assert!(predicate.eval(&span));
        let predicate = name(ends_with("test"));
        assert!(!predicate.eval(&span));
    }

    #[test]
    fn compound_predicates() {
        let predicate = target("tracing_capture")
            & name(eq("test_span"))
            & level(Level::INFO)
            & field("val", 42_u64);

        let mut span = CapturedSpan {
            metadata: METADATA,
            values: TracedValues::new(),
            stats: SpanStats::default(),
            events: vec![],
        };
        assert!(!predicate.eval(&span));
        let case = predicate.find_case(false, &span).unwrap();
        let products: Vec<_> = collect_products(&case);
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].name(), "fields.val");
        assert_eq!(products[0].value().to_string(), "None");

        span.values = TracedValues::from_iter([("val", 23_u64.into())]);
        let case = predicate.find_case(false, &span).unwrap();
        let products = collect_products(&case);
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].name(), "val");
        assert_eq!(products[0].value().to_string(), "UInt(23)");

        span.values = TracedValues::from_iter([("val", 42_u64.into())]);
        let eval = predicate.eval(&span);
        assert!(eval);
    }

    fn collect_products<'r>(case: &'r Case<'_>) -> Vec<&'r Product> {
        let mut cases = vec![case];
        let mut products = vec![];
        while !cases.is_empty() {
            products.extend(cases.iter().copied().flat_map(Case::products));
            cases = cases.into_iter().flat_map(Case::children).collect();
        }
        products
    }

    #[test]
    fn compound_predicates_combining_and_or() {
        let span = CapturedSpan {
            metadata: METADATA,
            values: TracedValues::from_iter([("val", "str".into())]),
            stats: SpanStats::default(),
            events: vec![],
        };

        let predicate = (target("tracing_capture") | field("val", 23_u64)) & level(Level::INFO);
        assert!(predicate.eval(&span));
        let case = predicate.find_case(true, &span).unwrap();
        let products = collect_products(&case);
        assert_eq!(products.len(), 2);
        let level_value = products[0].value().to_string();
        assert!(level_value.contains("Info"), "{level_value}");
        assert_eq!(
            products[1].value().to_string(),
            "tracing_capture::predicate"
        );

        let predicate = (target("tracing") | field("val", 23_u64)) & level(Level::INFO);
        assert!(!predicate.eval(&span));
        let case = predicate.find_case(false, &span).unwrap();
        let products = collect_products(&case);
        assert_eq!(products.len(), 2);
        assert_eq!(
            products[0].value().to_string(),
            "tracing_capture::predicate"
        );
        assert_eq!(products[1].value().to_string(), "String(\"str\")");
    }
}
