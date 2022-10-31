//! Extension trait for asserting against collections of `CapturedEvent`s and `CapturedSpan`s.

use predicates::Predicate;

use std::fmt;

use crate::Captured;

/// Helper to wrap iterators over [`CapturedSpan`]s or [`CapturedEvent`]s so that they are
/// more convenient to use with `Predicate`s.
///
/// See [the module-level docs](crate::predicates) for examples of usage.
///
/// [`CapturedSpan`]: crate::CapturedSpan
/// [`CapturedEvent`]: crate::CapturedEvent
pub trait ScannerExt: IntoIterator + Sized {
    /// Wraps this collection into a [`Scanner`].
    ///
    /// This call does not convert `self` into an iterator right away, but rather does so
    /// on each call to the `Scanner`. This means that a `Scanner` may be `Copy`able
    /// (e.g., in the case of slices).
    fn scanner(self) -> Scanner<Self>;
}

impl<T: Captured, I: IntoIterator<Item = T>> ScannerExt for I {
    fn scanner(self) -> Scanner<Self> {
        Scanner::new(self)
    }
}

/// Iterator extension that allows using `Predicate`s rather than closures to find matching
/// elements, and provides more informative error messages.
///
/// Returned by [`ScannerExt::scanner()`]; see its docs for more details.
#[derive(Debug, Clone, Copy)]
pub struct Scanner<I> {
    iter: I,
}

impl<I: IntoIterator> Scanner<I>
where
    I::Item: fmt::Debug,
{
    fn new(iter: I) -> Self {
        Self { iter }
    }

    /// Finds the single item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items, or multiple items match the predicate.
    pub fn single<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter.into_iter();
        let first = iter
            .find(|item| predicate.eval(item))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"));

        let second = iter.find(|item| predicate.eval(item));
        if let Some(second) = second {
            panic!(
                "multiple items match predicate {predicate}: {:#?}",
                [first, second]
            );
        }
        first
    }

    /// Finds the first item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items match the predicate.
    pub fn first<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter.into_iter();
        iter.find(|item| predicate.eval(item))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"))
    }

    /// Checks that all of the items match the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if any of items does not match the predicate.
    pub fn all<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) {
        let mut iter = self.iter.into_iter();
        if let Some(item) = iter.find(|item| !predicate.eval(item)) {
            panic!("item does not match predicate {predicate}: {item:#?}");
        }
    }

    /// Checks that none of the items match the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if any of items match the predicate.
    pub fn none<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) {
        let mut iter = self.iter.into_iter();
        if let Some(item) = iter.find(|item| predicate.eval(item)) {
            panic!("item matched predicate {predicate}: {item:#?}");
        }
    }
}

impl<I: IntoIterator> Scanner<I>
where
    I::Item: fmt::Debug,
    I::IntoIter: DoubleEndedIterator,
{
    /// Finds the last item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items match the predicate.
    pub fn last<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter.into_iter().rev();
        iter.find(|item| predicate.eval(item))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"))
    }
}
