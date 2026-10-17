//! A view's row cap: the first `n` rows of its order, or the last `n`.
//!
//! A tail reads rows in its declared order and shows the last `n` of them,
//! once the end of the set is known: always over a complete set, and over
//! windows once the source states a total. A source that pages windows and
//! never states one has no end to count back from, so its tail reads rows
//! answering the *flipped* order instead, rows `0..n` of them backwards.
//! Where that puts nulls and ties is the master's call, not the declared
//! order's.

use std::ops::Range;

use vantage_vista::SortDirection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cap {
    Limit(usize),
    /// The last `n` rows of the declared order, counted back from its end.
    Tail(usize),
    /// The first `n` rows of the flipped order, read backwards: a tail over
    /// windows of a set whose end is unknown.
    FlippedTail(usize),
}

impl Cap {
    pub(crate) fn n(self) -> usize {
        match self {
            Cap::Limit(n) | Cap::Tail(n) | Cap::FlippedTail(n) => n,
        }
    }

    pub(crate) fn is_tail(self) -> bool {
        !matches!(self, Cap::Limit(_))
    }

    /// Whether `len` shared rows fill the cap, so no further row belongs to
    /// the view.
    pub(crate) fn reached(self, len: usize) -> bool {
        len >= self.n()
    }

    /// Rows the view shows over `len` shared rows.
    pub(crate) fn view_len(self, len: usize) -> usize {
        len.min(self.n())
    }

    /// The shared index of view index `i` over `len` shared rows, or `None`
    /// beyond the cap.
    pub(crate) fn map(self, i: usize, len: usize) -> Option<usize> {
        let k = self.view_len(len);
        if i >= k {
            return None;
        }
        Some(match self {
            Cap::Limit(_) => i,
            Cap::Tail(_) => len - k + i,
            Cap::FlippedTail(_) => k - 1 - i,
        })
    }

    /// The view index shared index `s` shows at, or `None` outside the cap.
    pub(crate) fn view_index(self, s: usize, len: usize) -> Option<usize> {
        let k = self.view_len(len);
        match self {
            Cap::Limit(_) => (s < k).then_some(s),
            Cap::Tail(_) => (s >= len - k && s < len).then(|| s - (len - k)),
            Cap::FlippedTail(_) => (s < k).then(|| k - 1 - s),
        }
    }

    /// `range` clipped to the cap.
    pub(crate) fn clamp(self, range: Range<usize>) -> Range<usize> {
        range.start.min(self.n())..range.end.min(self.n())
    }

    /// The shared rows to ask for when the view asks for `loaded` over `len`
    /// shared rows. A limit asks for `loaded` clipped to the cap. A tail
    /// loads its whole cap: the last rows of the set, its first window while
    /// the end is unknown (`len == 0`), or rows `0..n` of the flipped order.
    /// Over listed two-pass rows, `loaded` mapped across only scopes which of
    /// them hydrate.
    pub(crate) fn shared_range(
        self,
        loaded: Range<usize>,
        len: usize,
        two_pass: bool,
    ) -> Range<usize> {
        let n = self.n();
        let k = self.view_len(len);
        let loaded = self.clamp(loaded);
        let scoped = two_pass && k > 0 && loaded.start < k;
        match self {
            Cap::Limit(_) => loaded,
            Cap::Tail(_) if len == 0 => 0..n,
            Cap::Tail(_) if scoped => (len - k + loaded.start)..(len - k + loaded.end.min(k)),
            Cap::Tail(_) => (len - k)..len,
            Cap::FlippedTail(_) if scoped => (k - loaded.end.min(k))..(k - loaded.start),
            Cap::FlippedTail(_) => 0..n,
        }
    }

    /// The order the rows load in for the view's declared `order`: flipped
    /// only for a flipped tail.
    pub(crate) fn load_order(
        self,
        order: Option<(String, SortDirection)>,
    ) -> Option<(String, SortDirection)> {
        match self {
            Cap::FlippedTail(_) => order.map(|(column, dir)| (column, flip(dir))),
            Cap::Limit(_) | Cap::Tail(_) => order,
        }
    }
}

fn flip(dir: SortDirection) -> SortDirection {
    match dir {
        SortDirection::Ascending => SortDirection::Descending,
        SortDirection::Descending => SortDirection::Ascending,
    }
}
