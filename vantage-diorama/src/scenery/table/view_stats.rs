//! `ViewStats` — what a table view is showing, as one comparable snapshot.
//!
//! A status bar, a menu badge and a dashboard counter all want the same few
//! numbers about a view: how many rows the table has, how many are loaded,
//! which ones are on screen, and what narrows or orders them. Reading those
//! one trait method at a time races the loader between calls; a snapshot is
//! taken in one place and published only when it differs from the last one.
//!
//! Read it with [`TableScenery::view_stats`]; wait for the next change on
//! [`TableScenery::subscribe_view_stats`].

mod publish;
#[cfg(test)]
mod tests;

use std::ops::Range;

use ciborium::Value as CborValue;
use vantage_vista::FilterOp;

use super::{LoadState, RowStatusSummary, SortDir, TableScenery};

pub(crate) use publish::{StatsCell, TotalKind};

/// Snapshot of a table view. Cheap to clone; compare with `==` to tell
/// whether anything a consumer renders has moved.
#[derive(Debug, Clone, Default, PartialEq)]
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
    /// Rows a displaying consumer declared on screen with
    /// [`set_shown_range`](super::TableScenery::set_shown_range), clamped to
    /// the rows the view has. `None` while no grid shows this view: a
    /// hydration [`set_viewport`](super::TableScenery::set_viewport) alone
    /// never sets it.
    pub showing: Option<Range<usize>>,
    /// More rows can be fetched beyond `loaded`.
    pub has_more: bool,
    /// Active sort column and direction.
    pub order: Option<(String, SortDir)>,
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
pub struct ViewFilter {
    pub column: String,
    pub op: FilterOp,
    pub value: CborValue,
    pub origin: FilterOrigin,
}

/// Where a [`ViewFilter`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOrigin {
    /// Part of what the view is: the Dio's base conditions and the
    /// builder's `where_eq` / `where_op`. A drill-down narrowed with
    /// [`Dio::with_condition_eq`](crate::Dio::with_condition_eq) lands here;
    /// one narrowed inside the master vista is invisible to the scenery.
    Query,
    /// Set at runtime by the consumer: filter chips
    /// ([`set_filters`](super::TableScenery::set_filters)) and the filter
    /// panel ([`set_filter_terms`](super::TableScenery::set_filter_terms)).
    View,
}

/// The single live-state marker for a view, most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewState {
    /// No load has finished; the view has no answer yet.
    #[default]
    Loading,
    /// Some rows failed to load or to write.
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
    pub fn derive(load: LoadState, rows: &RowStatusSummary, fetching: bool) -> Self {
        if load == LoadState::Loading {
            ViewState::Loading
        } else if rows.failed > 0 {
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

    /// The same view seen through a row cap of `cap`: once `cap` rows are
    /// loaded the view is complete at exactly `cap`.
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

    /// A snapshot built from the trait's own accessors, for implementations
    /// that keep no richer state. Knows nothing of order, search, query
    /// conditions or viewport.
    pub(crate) fn from_scenery<S: TableScenery + ?Sized>(scenery: &S) -> Self {
        let rows = scenery.status_summary();
        let load = scenery.load_state();
        ViewStats {
            total: scenery.estimated_total(),
            total_exact: load == LoadState::Complete,
            loaded: rows.loaded,
            showing: None,
            has_more: scenery.has_more(),
            order: None,
            search: None,
            filters: view_filters(&[], &[], &[], &scenery.filter_terms()),
            state: ViewState::derive(load, &rows, false),
            pending_writes: rows.pending_write,
            failed: rows.failed,
        }
    }
}

/// `range` clipped to `0..limit`; `None` when nothing of it remains.
pub(crate) fn clamp_range(range: Option<Range<usize>>, limit: usize) -> Option<Range<usize>> {
    range
        .map(|r| r.start.min(limit)..r.end.min(limit))
        .filter(|r| !r.is_empty())
}

/// Flatten a view's four filter sets into one list, query-level first.
pub(crate) fn view_filters(
    conditions: &[(String, CborValue)],
    op_conditions: &[super::OpCondition],
    ui_filters: &[(String, CborValue)],
    ui_terms: &[super::OpCondition],
) -> Vec<ViewFilter> {
    let eq = |(column, value): &(String, CborValue), origin| ViewFilter {
        column: column.clone(),
        op: FilterOp::Eq,
        value: value.clone(),
        origin,
    };
    let op = |c: &super::OpCondition, origin| ViewFilter {
        column: c.column.clone(),
        op: c.op,
        value: c.value.clone(),
        origin,
    };
    conditions
        .iter()
        .map(|c| eq(c, FilterOrigin::Query))
        .chain(op_conditions.iter().map(|c| op(c, FilterOrigin::Query)))
        .chain(ui_filters.iter().map(|c| eq(c, FilterOrigin::View)))
        .chain(ui_terms.iter().map(|c| op(c, FilterOrigin::View)))
        .collect()
}
