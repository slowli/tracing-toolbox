use id_arena::{DefaultArenaBehavior, Id};

use std::{iter::FlatMap, slice};

use crate::{CapturedEvent, CapturedEventInner, CapturedSpan, CapturedSpanInner, Storage};

#[derive(Debug)]
enum IdsIter<'a, T> {
    Arena(id_arena::Iter<'a, T, DefaultArenaBehavior<T>>),
    Slice(slice::Iter<'a, Id<T>>),
}

/// Iterator over [`CapturedSpan`]s returned from [`Storage::all_spans()`] etc.
#[derive(Debug)]
pub struct CapturedSpans<'a> {
    storage: &'a Storage,
    ids_iter: IdsIter<'a, CapturedSpanInner>,
}

impl<'a> CapturedSpans<'a> {
    pub(crate) fn from_slice(storage: &'a Storage, ids: &'a [Id<CapturedSpanInner>]) -> Self {
        Self {
            storage,
            ids_iter: IdsIter::Slice(ids.iter()),
        }
    }

    pub(crate) fn from_arena(storage: &'a Storage) -> Self {
        Self {
            storage,
            ids_iter: IdsIter::Arena(storage.spans.iter()),
        }
    }
}

impl<'a> Iterator for CapturedSpans<'a> {
    type Item = CapturedSpan<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.ids_iter {
            IdsIter::Arena(arena) => {
                let (_, inner) = arena.next()?;
                Some(CapturedSpan {
                    inner,
                    storage: self.storage,
                })
            }
            IdsIter::Slice(slice) => {
                let id = *slice.next()?;
                Some(self.storage.span(id))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.ids_iter {
            IdsIter::Arena(arena) => arena.size_hint(),
            IdsIter::Slice(slice) => slice.size_hint(),
        }
    }
}

impl DoubleEndedIterator for CapturedSpans<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.ids_iter {
            IdsIter::Arena(arena) => {
                let (_, inner) = arena.next_back()?;
                Some(CapturedSpan {
                    inner,
                    storage: self.storage,
                })
            }
            IdsIter::Slice(slice) => {
                let id = *slice.next_back()?;
                Some(self.storage.span(id))
            }
        }
    }
}

impl ExactSizeIterator for CapturedSpans<'_> {
    fn len(&self) -> usize {
        match &self.ids_iter {
            IdsIter::Arena(arena) => arena.len(),
            IdsIter::Slice(slice) => slice.len(),
        }
    }
}

/// Iterator over [`CapturedEvent`]s returned from [`Storage::all_events()`] etc.
#[derive(Debug)]
pub struct CapturedEvents<'a> {
    storage: &'a Storage,
    ids_iter: IdsIter<'a, CapturedEventInner>,
}

impl<'a> CapturedEvents<'a> {
    pub(crate) fn from_slice(storage: &'a Storage, ids: &'a [Id<CapturedEventInner>]) -> Self {
        Self {
            storage,
            ids_iter: IdsIter::Slice(ids.iter()),
        }
    }

    pub(crate) fn from_arena(storage: &'a Storage) -> Self {
        Self {
            storage,
            ids_iter: IdsIter::Arena(storage.events.iter()),
        }
    }
}

impl<'a> Iterator for CapturedEvents<'a> {
    type Item = CapturedEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.ids_iter {
            IdsIter::Arena(arena) => {
                let (_, inner) = arena.next()?;
                Some(CapturedEvent {
                    inner,
                    storage: self.storage,
                })
            }
            IdsIter::Slice(slice) => {
                let id = *slice.next()?;
                Some(self.storage.event(id))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.ids_iter {
            IdsIter::Arena(arena) => arena.size_hint(),
            IdsIter::Slice(slice) => slice.size_hint(),
        }
    }
}

impl DoubleEndedIterator for CapturedEvents<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.ids_iter {
            IdsIter::Arena(arena) => {
                let (_, inner) = arena.next_back()?;
                Some(CapturedEvent {
                    inner,
                    storage: self.storage,
                })
            }
            IdsIter::Slice(slice) => {
                let id = *slice.next_back()?;
                Some(self.storage.event(id))
            }
        }
    }
}

impl ExactSizeIterator for CapturedEvents<'_> {
    fn len(&self) -> usize {
        match &self.ids_iter {
            IdsIter::Arena(arena) => arena.len(),
            IdsIter::Slice(slice) => slice.len(),
        }
    }
}

/// Iterator over descendant [`CapturedSpan`]s of a span.
/// Returned by [`CapturedSpan::descendants()`].
#[derive(Debug)]
pub struct DescendantSpans<'a> {
    storage: &'a Storage,
    layers: Vec<&'a [Id<CapturedSpanInner>]>,
}

impl<'a> DescendantSpans<'a> {
    pub(crate) fn new(root: &CapturedSpan<'a>) -> Self {
        Self {
            storage: root.storage,
            layers: vec![&root.inner.child_ids],
        }
    }
}

impl<'a> Iterator for DescendantSpans<'a> {
    type Item = CapturedSpan<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let last_layer = self.layers.last_mut()?;
            if let Some((&head, tail)) = last_layer.split_first() {
                let span = self.storage.span(head);
                *last_layer = tail;
                if !span.inner.child_ids.is_empty() {
                    self.layers.push(&span.inner.child_ids);
                }
                break Some(span);
            }
            // The last layer is empty at this point.
            self.layers.pop();
        }
    }
}

/// Iterator over the descendant [events](CapturedEvent) of a [`CapturedSpan`].
/// Returned by [`CapturedSpan::descendant_events()`].
#[derive(Debug)]
pub struct DescendantEvents<'a> {
    inner: FlatMap<
        DescendantSpans<'a>,
        CapturedEvents<'a>,
        fn(CapturedSpan<'a>) -> CapturedEvents<'a>,
    >,
}

impl<'a> DescendantEvents<'a> {
    pub(crate) fn new(root: &CapturedSpan<'a>) -> Self {
        Self {
            inner: root.descendants().flat_map(|span| span.events()),
        }
    }
}

impl<'a> Iterator for DescendantEvents<'a> {
    type Item = CapturedEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
