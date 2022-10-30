//! `level()` predicate factory.

use predicates::{
    reflection::{Case, PredicateReflection},
    Predicate,
};
use tracing_core::{Level, LevelFilter};

use std::fmt;

use crate::{CapturedEvent, CapturedSpan};

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
/// let _ = storage.spans().find_single(level(Level::INFO));
/// let _ = storage.spans().find_single(level(LevelFilter::DEBUG));
/// let _ = storage.spans().find_single(level([gt(Level::WARN)]));
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

impl_bool_ops!(LevelPredicate<P>);

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
