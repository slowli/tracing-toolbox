//! Extension trait for asserting against collections of `CapturedEvent`s and `CapturedSpan`s.

use std::fmt;

use predicates::Predicate;

use crate::{CapturedEvent, CapturedEvents, CapturedSpan, CapturedSpans, DescendantSpans, Storage};

/// Helper to wrap holders of [`CapturedSpan`]s or [`CapturedEvent`]s
/// (spans or the underlying [`Storage`]) so that they are more convenient to use with `Predicate`s.
///
/// See [the module-level docs](crate::predicates) for examples of usage.
///
/// [`CapturedEvent`]: crate::CapturedEvent
pub trait ScanExt<'a>: Sized {
    /// Creates a scanner for the spans contained by this holder. What is meant by "contained"
    /// (i.e., whether the scan is deep or shallow), depends on the holder type and is documented
    /// at the corresponding impl.
    fn scan_spans(self) -> Scanner<Self, CapturedSpans<'a>>;
    /// Creates a scanner for the events contained by this holder. What is meant by "contained"
    /// (i.e., whether the scan is deep or shallow), depends on the holder type and is documented
    /// at the corresponding impl.
    fn scan_events(self) -> Scanner<Self, CapturedEvents<'a>>;
}

/// Scans for `Storage` are deep; they include *all* captured spans / events, not just root ones.
impl<'a> ScanExt<'a> for &'a Storage {
    fn scan_spans(self) -> Scanner<Self, CapturedSpans<'a>> {
        Scanner::new(self, Storage::all_spans)
    }

    fn scan_events(self) -> Scanner<Self, CapturedEvents<'a>> {
        Scanner::new(self, Storage::all_events)
    }
}

/// Scans for `CapturedSpan` are shallow, i.e. include only direct children spans / events.
impl<'a> ScanExt<'a> for CapturedSpan<'a> {
    fn scan_spans(self) -> Scanner<Self, CapturedSpans<'a>> {
        Scanner::new(self, |span| span.children())
    }

    fn scan_events(self) -> Scanner<Self, CapturedEvents<'a>> {
        Scanner::new(self, |span| span.events())
    }
}

impl<'a> CapturedSpan<'a> {
    /// Deeply scans all descendants of this span.
    pub fn deep_scan_spans(self) -> Scanner<Self, DescendantSpans<'a>> {
        Scanner::new(self, |span| span.descendants())
    }

    /// Deeply scans all descendant events of this span.
    pub fn deep_scan_events(self) -> Scanner<Self, impl Iterator<Item = CapturedEvent<'a>> + 'a> {
        Scanner::new(self, |span| span.events().chain(span.descendant_events()))
    }
}

/// Helper that allows using `Predicate`s rather than closures to find matching elements,
/// and provides more informative error messages.
///
/// Returned by the [`ScanExt`] methods; see its docs for more details.
#[derive(Debug)]
pub struct Scanner<T, I> {
    items: T,
    into_iter: fn(T) -> I,
}

impl<T: Clone, I> Clone for Scanner<T, I> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            into_iter: self.into_iter,
        }
    }
}

impl<T: Copy, I> Copy for Scanner<T, I> {}

impl<T, I> Scanner<T, I>
where
    I: Iterator,
    I::Item: fmt::Debug,
{
    fn new(items: T, into_iter: fn(T) -> I) -> Self {
        Self { items, into_iter }
    }

    fn iter(self) -> I {
        (self.into_iter)(self.items)
    }

    /// Finds the single item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items, or multiple items match the predicate.
    pub fn single<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter();
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
        let mut iter = self.iter();
        iter.find(|item| predicate.eval(item))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"))
    }

    /// Checks that all of the items match the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if any of items does not match the predicate.
    pub fn all<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) {
        let mut iter = self.iter();
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
        let mut iter = self.iter();
        if let Some(item) = iter.find(|item| predicate.eval(item)) {
            panic!("item matched predicate {predicate}: {item:#?}");
        }
    }
}

impl<T, I> Scanner<T, I>
where
    I: DoubleEndedIterator,
    I::Item: fmt::Debug,
{
    /// Finds the last item matching the predicate.
    ///
    /// # Panics
    ///
    /// Panics with an informative message if no items match the predicate.
    pub fn last<P: Predicate<I::Item> + ?Sized>(self, predicate: &P) -> I::Item {
        let mut iter = self.iter().rev();
        iter.find(|item| predicate.eval(item))
            .unwrap_or_else(|| panic!("no items have matched predicate {predicate}"))
    }
}
