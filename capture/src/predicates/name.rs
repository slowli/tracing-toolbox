//! `name()` predicate factory.

use predicates::{
    reflection::{Case, PredicateReflection},
    Predicate,
};

use std::fmt;

use crate::CapturedSpan;

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
/// # use tracing_capture::{predicates::{name, ScannerExt}, CaptureLayer, SharedStorage};
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
/// let spans = storage.all_spans().scanner();
/// let _ = spans.single(&name(eq("compute")));
/// let _ = spans.single(&name(starts_with("co")));
/// ```
pub fn name<P: Predicate<str>>(matches: P) -> NamePredicate<P> {
    NamePredicate { matches }
}

/// Predicate for the name of a [`CapturedSpan`] returned by the [`name()`] function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamePredicate<P> {
    matches: P,
}

impl_bool_ops!(NamePredicate<P>);

impl<P: Predicate<str>> fmt::Display for NamePredicate<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "name({})", self.matches)
    }
}

impl<P: Predicate<str>> PredicateReflection for NamePredicate<P> {}

impl<P: Predicate<str>> Predicate<CapturedSpan<'_>> for NamePredicate<P> {
    fn eval(&self, variable: &CapturedSpan<'_>) -> bool {
        self.matches.eval(variable.metadata().name())
    }

    fn find_case(&self, expected: bool, variable: &CapturedSpan<'_>) -> Option<Case<'_>> {
        let child = self
            .matches
            .find_case(expected, variable.metadata().name())?;
        Some(Case::new(Some(self), expected).add_child(child))
    }
}
