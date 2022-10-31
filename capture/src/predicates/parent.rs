//! `parent()` predicate factory.

use predicates::{
    reflection::{Case, PredicateReflection, Product},
    Predicate,
};

use std::fmt;

use crate::{CapturedEvent, CapturedSpan};

/// Creates a predicate for the direct parent [`CapturedSpan`] of a span or a [`CapturedEvent`].
///
/// # Examples
///
/// ```
/// # use predicates::ord::eq;
/// # use tracing_core::Level;
/// # use tracing_subscriber::{layer::SubscriberExt, Registry};
/// # use tracing_capture::{predicates::*, CaptureLayer, SharedStorage};
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
/// let parent_pred = level(Level::INFO) & name(eq("compute"));
/// let _ = storage.scan_events().single(&parent(parent_pred));
/// ```
pub fn parent<P>(matches: P) -> ParentPredicate<P>
where
    P: for<'a> Predicate<CapturedSpan<'a>>,
{
    ParentPredicate { matches }
}

/// Predicate for the parent of a [`CapturedSpan`] or [`CapturedEvent`] returned
/// by the [`parent()`] function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentPredicate<P> {
    matches: P,
}

impl_bool_ops!(ParentPredicate<P>);

impl<P> fmt::Display for ParentPredicate<P>
where
    P: for<'a> Predicate<CapturedSpan<'a>>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "parent({})", self.matches)
    }
}

impl<P> PredicateReflection for ParentPredicate<P> where P: for<'a> Predicate<CapturedSpan<'a>> {}

macro_rules! impl_parent_predicate {
    ($target:ty) => {
        impl<P> Predicate<$target> for ParentPredicate<P>
        where
            P: for<'a> Predicate<CapturedSpan<'a>>,
        {
            fn eval(&self, variable: &$target) -> bool {
                let parent = variable.parent();
                parent.map_or(false, |parent| self.matches.eval(&parent))
            }

            fn find_case(&self, expected: bool, variable: &$target) -> Option<Case<'_>> {
                let parent = variable.parent();
                let parent = if let Some(parent) = parent {
                    parent
                } else {
                    return if expected {
                        None // was expecting a parent, but there is none
                    } else {
                        let product = Product::new("parent", "None");
                        Some(Case::new(Some(self), expected).add_product(product))
                    };
                };

                let child = self.matches.find_case(expected, &parent)?;
                Some(Case::new(Some(self), expected).add_child(child))
            }
        }
    };
}

impl_parent_predicate!(CapturedSpan<'_>);
impl_parent_predicate!(CapturedEvent<'_>);
