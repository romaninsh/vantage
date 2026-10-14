//! `ViewStats` — what a table view is showing, as one comparable snapshot.
//!
//! A status bar, a menu badge and a dashboard counter all want the same few
//! numbers about a view: how many rows the table has, how many are loaded,
//! which ones are on screen, and what narrows or orders them. Reading those
//! one trait method at a time races the loader between calls; a snapshot is
//! taken in one place and published only when it differs from the last one.
//!
//! Read it with [`TableScenery::view_stats`](crate::TableScenery::view_stats);
//! wait for the next change on
//! [`TableScenery::subscribe_view_stats`](crate::TableScenery::subscribe_view_stats).

mod publish;
#[cfg(test)]
mod tests;

use std::ops::Range;

use ciborium::Value as CborValue;
use vantage_vista::{Condition, FilterOp, SortDirection};

use super::{LoadState, RowStatusSummary};

pub(crate) use publish::{StatsCell, TotalKind};

/// Snapshot of a table view. Cheap to clone; compare with `==` to tell
/// whether anything a consumer renders has moved.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ViewStats {
    /// Rows the table has, when known. `None` when only a lower bound is
    /// known — see [`has_more`](Self::has_more).
    pub total: Option<usize>,
    /// Whether `total` is a count (a source-stated total, the end of the set
    /// reached) rather than an estimate (a total remembered from an earlier
    /// session that has not been re-stated yet).
    pub total_exact: bool,
    /// Rows fetched into this view so far.
    pub loaded: usize,
    /// Rows a displaying consumer declared on screen, the `shown` argument
    /// of [`set_viewport`](super::TableScenery::set_viewport), clamped to
    /// the rows the view has. `None` while no grid shows this view: a
    /// viewport with an empty `shown` loads rows without setting it.
    pub showing: Option<Range<usize>>,
    /// More rows can be fetched beyond `loaded`.
    pub has_more: bool,
    /// Active sort column and direction.
    pub order: Option<(String, SortDirection)>,
    /// Active quicksearch text.
    pub search: Option<String>,
    /// Every term narrowing the view, query-level first.
    pub filters: Vec<ViewFilter>,
    /// The one live-state marker to show.
    pub state: ViewState,
    /// Rows with an optimistic write still in flight.
    pub pending_writes: usize,
    /// Rows whose detail load or write failed.
    pub failed: usize,
}

/// One term narrowing a view.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ViewFilter {
    pub column: String,
    pub op: FilterOp,
    pub value: CborValue,
    pub origin: FilterOrigin,
}

/// Where a [`ViewFilter`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterOrigin {
    /// Part of what the view is: the Dio's residual conditions and the
    /// builder's [`filter`](super::TableSceneryBuilder::filter)s. A condition
    /// a derived Dio's master accepted (see
    /// [`Dio::derive`](crate::Dio::derive)) is applied inside the
    /// master and is invisible to the scenery.
    Query,
    /// Set at runtime by the consumer through
    /// [`set_filters`](super::TableScenery::set_filters).
    View,
}

/// The single live-state marker for a view, most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ViewState {
    /// No load has finished; the view has no answer yet.
    #[default]
    Loading,
    /// The view can't answer its query ([`LoadState::Failed`]), or some rows
    /// failed to load or to write.
    Failed,
    /// Some rows carry an optimistic write still in flight.
    PendingWrites,
    /// A fetch is in flight over rows already shown — what is on screen
    /// answers the previous query or refresh.
    Stale,
    /// Nothing in flight, nothing failed.
    Ready,
}

impl ViewState {
    /// Pick the marker from the load state, the row breakdown and whether a
    /// fetch is in flight.
    pub fn derive(load: &LoadState, rows: &RowStatusSummary, fetching: bool) -> Self {
        if *load == LoadState::Loading {
            ViewState::Loading
        } else if matches!(load, LoadState::Failed(_)) || rows.failed > 0 {
            ViewState::Failed
        } else if rows.pending_write > 0 {
            ViewState::PendingWrites
        } else if fetching {
            ViewState::Stale
        } else {
            ViewState::Ready
        }
    }
}

/// The default way to state how many rows a view has, which never presents
/// a partial number as the whole table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ViewCount {
    /// The total is a count: `1,240`.
    Exact(usize),
    /// The total is an estimate: `~1,200`.
    Estimated(usize),
    /// No total, more rows to fetch — the loaded rows are a lower bound: `300+`.
    AtLeast(usize),
    /// No total and nothing more to fetch — the loaded rows are all there is.
    Loaded(usize),
}

impl ViewCount {
    /// The number, without its qualifier.
    pub fn value(self) -> usize {
        match self {
            ViewCount::Exact(n)
            | ViewCount::Estimated(n)
            | ViewCount::AtLeast(n)
            | ViewCount::Loaded(n) => n,
        }
    }
}

impl std::fmt::Display for ViewCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = crate::debug::num(self.value());
        match self {
            ViewCount::Exact(_) | ViewCount::Loaded(_) => f.write_str(&n),
            ViewCount::Estimated(_) => write!(f, "~{n}"),
            ViewCount::AtLeast(_) => write!(f, "{n}+"),
        }
    }
}

impl ViewStats {
    /// The default count rule: an exact total, else an estimated one, else
    /// the loaded rows marked as a lower bound while more exist, else the
    /// loaded rows.
    pub fn count(&self) -> ViewCount {
        match self.total {
            Some(n) if self.total_exact => ViewCount::Exact(n),
            Some(n) => ViewCount::Estimated(n),
            None if self.has_more => ViewCount::AtLeast(self.loaded),
            None => ViewCount::Loaded(self.loaded),
        }
    }

    /// [`count`](Self::count) as display text: `1,240`, `~1,200`, `300+`.
    pub fn count_text(&self) -> String {
        self.count().to_string()
    }

    /// Number of rows in [`showing`](Self::showing); 0 when none.
    pub fn showing_count(&self) -> usize {
        self.showing_len().unwrap_or(0)
    }

    /// Number of rows in [`showing`](Self::showing); `None` while no grid
    /// shows the view.
    pub fn showing_len(&self) -> Option<usize> {
        self.showing.as_ref().map(|r| r.len())
    }

    /// The same view seen through a [`limit`](super::TableSceneryBuilder::limit)
    /// or [`tail`](super::TableSceneryBuilder::tail) of `cap`: once `cap`
    /// rows are loaded the view is complete at exactly `cap`.
    pub fn capped(mut self, cap: usize) -> Self {
        if self.loaded >= cap {
            self.loaded = cap;
            self.total = Some(cap);
            self.total_exact = true;
            self.has_more = false;
        } else {
            self.total = self.total.map(|t| t.min(cap));
        }
        self.showing = clamp_range(self.showing, cap);
        self
    }
}

/// `range` clipped to `0..limit`; `None` when nothing of it remains.
pub(crate) fn clamp_range(range: Option<Range<usize>>, limit: usize) -> Option<Range<usize>> {
    range
        .map(|r| r.start.min(limit)..r.end.min(limit))
        .filter(|r| !r.is_empty())
}

/// Flatten a view's filter sets into one list: the query-level `query`
/// conditions first, then the runtime `filters`.
pub(crate) fn view_filters(query: &[Condition], filters: &[Condition]) -> Vec<ViewFilter> {
    let entry = |c: &Condition, origin| ViewFilter {
        column: c.column.clone(),
        op: c.op,
        value: c.value.clone(),
        origin,
    };
    query
        .iter()
        .map(|c| entry(c, FilterOrigin::Query))
        .chain(filters.iter().map(|c| entry(c, FilterOrigin::View)))
        .collect()
}
