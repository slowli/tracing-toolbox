//! Extension trait for asserting against collections of `CapturedEvent`s and `CapturedSpan`s.

use predicates::Predicate;

use std::{borrow::Borrow, fmt, marker::PhantomData};

use crate::Captured;

/// Helper to wrap iterators over [`CapturedSpan`]s or [`CapturedEvent`]s so that they are
/// more convenient to use with `Predicate`s.
///
/// See [the module-level docs](crate::predicates) for examples of usage.
///
/// [`CapturedSpan`]: crate::CapturedSpan
/// [`CapturedEvent`]: crate::CapturedEvent
pub trait ScannerExt: IntoIterator + Sized {
    /// Type argument for the `Predicates` that will be used in the created [`Scanner`].
    type PredicateArg: Captured;
    /// Wraps this collection into a [`Scanner`].
    ///
    /// This call does not convert `self` into an iterator right away, but rather does so
    /// on each call to the `Scanner`. This means that a `Scanner` may be `Copy`able
    /// (e.g., in the case of slices).
    fn scanner(self) -> Scanner<Self::PredicateArg, Self>;
}

impl<'a, T: Captured, I: 'a + IntoIterator<Item = &'a T>> ScannerExt for I {
    type PredicateArg = T;

    fn scanner(self) -> Scanner<Self::PredicateArg, Self> {
        Scanner::new(self)
    }
}

/// Iterator extension that allows using `Predicate`s rather than closures to find matching
/// elements, and provides more informative error messages.
///
/// Returned by [`ScannerExt::scanner()`]; see its docs for more details.
#[derive(Debug)]
pub struct Scanner<Item: ?Sized, I> {
    iter: I,
    _item: PhantomData<fn() -> Item>,
}

impl<Item: ?Sized, I: Clone> Clone for Scanner<Item, I> {
    fn clone(&self) -> Self {
        Self {
            iter: self.iter.clone(),
            _item: PhantomData,
        }
    }
}

impl<Item: ?Sized, I: Copy> Copy for Scanner<Item, I> {}

impl<Item, I: IntoIterator> Scanner<Item, I>
where
    Item: fmt::Debug + ?Sized,
    I::Item: Borrow<Item>,
{
    fn new(iter: I) -> Self {
        Self {
            iter,
            _item: PhantomData,
        }
    }

    /// Finds the single item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items, or multiple items match the predicate.
    pub fn single<P: Predicate<Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter.into_iter();
        let first = iter
            .find(|item| predicate.eval(item.borrow()))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"));

        let second = iter.find(|item| predicate.eval(item.borrow()));
        if let Some(second) = second {
            panic!(
                "multiple items match predicate {predicate}: {:#?}",
                [first.borrow(), second.borrow()]
            );
        }
        first
    }

    /// Finds the first item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items match the predicate.
    pub fn first<P: Predicate<Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter.into_iter();
        iter.find(|item| predicate.eval(item.borrow()))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"))
    }
}

impl<Item, I: IntoIterator> Scanner<Item, I>
where
    Item: fmt::Debug + ?Sized,
    I::Item: Borrow<Item>,
    I::IntoIter: DoubleEndedIterator,
{
    /// Finds the last item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items match the predicate.
    pub fn last<P: Predicate<Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter.into_iter().rev();
        iter.find(|item| predicate.eval(item.borrow()))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"))
    }
}
