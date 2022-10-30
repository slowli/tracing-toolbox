//! `target()` predicate factory.

use predicates::{
    reflection::{Case, PredicateReflection, Product},
    Predicate,
};

use std::fmt;

use crate::{CapturedEvent, CapturedSpan};

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

impl<'a> IntoTargetPredicate for &'a str {
    type Predicate = TargetStrPredicate<'a>;

    fn into_predicate(self) -> Self::Predicate {
        TargetStrPredicate { prefix: self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetStrPredicate<'a> {
    prefix: &'a str,
}

impl fmt::Display for TargetStrPredicate<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "target ^= {}", self.prefix)
    }
}

impl PredicateReflection for TargetStrPredicate<'_> {}

impl Predicate<str> for TargetStrPredicate<'_> {
    fn eval(&self, variable: &str) -> bool {
        variable
            .strip_prefix(self.prefix)
            .map_or(false, |stripped| {
                stripped.is_empty() || stripped.starts_with("::")
            })
    }

    fn find_case(&self, expected: bool, variable: &str) -> Option<Case<'_>> {
        if self.eval(variable) == expected {
            let product = Product::new("target", variable.to_owned());
            Some(Case::new(Some(self), expected).add_product(product))
        } else {
            None
        }
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
/// # use tracing_capture::{predicates::{target, ScannerExt}, CaptureLayer, SharedStorage};
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
/// let spans = storage.spans().scanner();
/// let _ = spans.single(&target("capture"));
/// let _ = spans.single(&target([starts_with("cap")]));
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

impl_bool_ops!(TargetPredicate<P>);

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
