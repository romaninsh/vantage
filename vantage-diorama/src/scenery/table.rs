//! `TableScenery` — reactive ordered-rows view onto a Dio.
//!
//! The Scenery holds a sparse
//! `BTreeMap<usize, Arc<EnrichedRecord>>` keyed by row index. Rows
//! arrive via two paths:
//!
//! - **Cache seed** — `open()` reads whatever is already in the cache
//!   (e.g. warmed from disk on restart) and assigns indices in
//!   iteration order. Subsequent `DatasetChanged` / `Refreshing` events
//!   re-seed the same way.
//! - **Chunk load** — `set_viewport` / `request_load_more` queue a
//!   range request on a debounce channel; on commit, the Lens's paged
//!   loader fetches the missing indices from the
//!   master and streams them back through [`ChunkSink`](crate::ChunkSink).
//!
//! The reactor task ignores the scenery's own viewport events
//! (`ViewportChanged`, `RangeLoaded`, `LoadFailed`) to avoid looping
//! on its own output.

mod builder;
mod capped;
mod count;
mod handle;
mod helpers;
mod load_failure;
mod loader;
mod plan;
mod reactor;
mod row_map;
mod state;
mod two_pass;
mod view_stats;

use std::ops::Range;
use std::sync::Arc;

use tokio::sync::watch;
use vantage_vista::VistaCapabilities;

use crate::dio::Generation;

use super::enriched_record::EnrichedRecord;

pub use builder::TableSceneryBuilder;
pub use capped::CappedScenery;
pub(crate) use handle::compute_load_state;
pub use load_failure::{Clause, LoadFailure};
pub use view_stats::{FilterOrigin, ViewCount, ViewFilter, ViewState, ViewStats};

/// UI-side sort direction. Mirrors `vantage_vista::SortDirection` but
/// kept distinct so Scenery callers don't need to import vista types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    pub(crate) fn to_vista(self) -> vantage_vista::SortDirection {
        match self {
            SortDir::Asc => vantage_vista::SortDirection::Ascending,
            SortDir::Desc => vantage_vista::SortDirection::Descending,
        }
    }
}

/// A non-equality filter condition applied **locally** over the cached rows —
/// the fallback path when the master vista can't push the operator into its
/// query (advertised via
/// [`Capability::FilterOps`](vantage_vista::Capability::FilterOps)).
/// `value` carries a scalar for the scalar operators and a
/// [`CborValue::Array`](ciborium::Value::Array) for `InSet` / `NotInSet`.
/// Plain equality keeps using the lighter `where_eq` / `conditions` path.
#[derive(Debug, Clone)]
pub struct OpCondition {
    pub column: String,
    pub op: vantage_vista::FilterOp,
    pub value: ciborium::Value,
}

impl OpCondition {
    pub fn new(
        column: impl Into<String>,
        op: vantage_vista::FilterOp,
        value: impl Into<ciborium::Value>,
    ) -> Self {
        Self {
            column: column.into(),
            op,
            value: value.into(),
        }
    }

    /// Stable `field<sym>value` fragment for the query-variant cache key, so a
    /// scenery narrowed by this operator gets its own ordered index (never
    /// colliding with the unfiltered set or a different-operator filter).
    pub fn key_fragment(&self) -> String {
        format!("{}{}{:?}", self.column, self.op.key_symbol(), self.value)
    }
}

/// Internal viewport request carried over the debounce channel.
#[derive(Debug, Clone)]
pub(crate) struct ViewportRequest {
    pub(crate) range: Range<usize>,
    /// `request_load_more` sets this true so a fully-cached range
    /// still triggers a fetch (paging past the cache end).
    pub(crate) force_load: bool,
    /// Who is waiting. A viewport or a load-more is someone looking at the
    /// rows; a refresh over cached rows is nobody. The loader scopes the
    /// chunk callback with it, so the transport picks its retry policy
    /// without a parameter.
    pub(crate) priority: vantage_core::Priority,
}

/// How much of a scenery's data has arrived.
///
/// Distinguishes the three situations a consumer has to act on differently. A
/// grid renders a skeleton for [`Loading`](Self::Loading) and its empty-set
/// placeholder only for a [`Complete`](Self::Complete) view with no rows — the
/// same picture, opposite meanings. A test or a profiler waits for `Loading` to
/// end before asserting, and for `Complete` before claiming a total.
/// [`Failed`](Self::Failed) means the view has no honest answer for its
/// query and shows no rows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoadState {
    /// No load has finished. The view has no answer yet — not even "empty".
    Loading,
    /// Rows are present and more are known to exist: the ordinary state of a
    /// paged grid that has loaded its first window of a larger set.
    Partial,
    /// Every row this query yields is in the view.
    Complete,
    /// The view can't answer its current query: the master rejected a
    /// clause, or refused one the view has no complete set to evaluate
    /// over. The view holds no rows until the query changes.
    Failed(LoadFailure),
}

impl LoadState {
    /// Bare-word rendering for the debug stream's `"state"` line — passed as
    /// a plain `&str` field (not `?self`) so the capture harness Debug-quotes
    /// it (`to="Complete"`), matching every other string field this stream
    /// emits.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            LoadState::Loading => "loading",
            LoadState::Partial => "partial",
            LoadState::Complete => "complete",
            LoadState::Failed(_) => "failed",
        }
    }
}

/// Breakdown of the row statuses currently materialized in a scenery's sparse
/// map, kept current as rows are written, so reading it costs nothing per
/// row. The per-scenery slice of the diagnostics surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowStatusSummary {
    /// Rows actually present in the sparse map (a paged scenery's `row_count`
    /// can be far larger — most indices are unloaded).
    pub loaded: usize,
    pub fresh: usize,
    pub incomplete: usize,
    pub pending_write: usize,
    /// `LoadFailed` + `WriteFailed` combined.
    pub failed: usize,
}

/// Reactive view onto a Dio that exposes an ordered, paginated row set.
pub trait TableScenery: Send + Sync {
    fn row_count(&self) -> usize;

    /// How much of this view's data has arrived.
    ///
    /// Three states, because two are not enough to act on: a consumer needs to
    /// tell "no rows" from "no rows *yet*" (they render identically and mean
    /// opposite things), and a test or a profiler needs to tell "showing
    /// something" from "showing everything" before it measures.
    fn load_state(&self) -> LoadState {
        LoadState::Complete
    }

    /// Shorthand for [`LoadState::Loading`] — the view has no answer yet.
    fn is_loading(&self) -> bool {
        self.load_state() == LoadState::Loading
    }

    /// Status breakdown over the rows currently in the sparse map. Used by the
    /// diagnostics surface to report how much of a scenery is hydrated.
    fn status_summary(&self) -> RowStatusSummary;
    fn has_more(&self) -> bool;
    fn estimated_total(&self) -> Option<usize>;
    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>>;

    /// Declare the rows to hydrate. Any consumer may call it — a grid, a
    /// dashboard counter, an observation probe — and it does not change what
    /// [`ViewStats::showing`] reports; see [`set_shown_range`](Self::set_shown_range).
    fn set_viewport(&self, range: Range<usize>);

    /// Declare the rows a displaying consumer (a grid) has on screen, or
    /// `None` when it shows none — closed, hidden or scrolled to nothing.
    /// The only input to [`ViewStats::showing`]. Kept apart from
    /// [`set_viewport`](Self::set_viewport) so a consumer that loads rows
    /// without displaying them doesn't claim the status bar's "Showing a–b".
    /// A grid typically calls both with the same range. Default no-op.
    fn set_shown_range(&self, _range: Option<Range<usize>>) {}

    fn request_load_more(&self);
    fn request_refresh(&self);
    fn set_sort(&self, column: Option<String>, dir: SortDir);
    /// Replace the UI-level equality filter set (grid filter chips) —
    /// `column == value` terms ANDed together and with the query's own
    /// conditions. A paged scenery carries them into every chunk fetch; an
    /// eager one evaluates them over its cache (which is what lets them
    /// reference augmented columns). Empty vec clears.
    fn set_filters(&self, filters: Vec<(String, ciborium::Value)>);

    /// Replace the UI-level operator filter set (the grid's filter panel):
    /// `column <op> value` terms ANDed together and with the query's own
    /// conditions. Same mechanics as [`set_search`](Self::set_search): a
    /// paged scenery carries the terms into every chunk fetch and the master
    /// takes what it can — equality always, richer operators when it
    /// advertises `FilterOps` — handing the rest back as the view's residual;
    /// an eager scenery evaluates them over its complete cache. Empty vec
    /// clears. Default no-op for wrappers that don't filter.
    fn set_filter_terms(&self, _terms: Vec<OpCondition>) {}

    /// The operator filter set currently in force — what a chip strip or a
    /// persisted-filter store reads, so there is one list, held here.
    fn filter_terms(&self) -> Vec<OpCondition> {
        Vec::new()
    }

    /// How many rows this view would hold with `terms` in place of its
    /// current UI filter terms. The view's other clauses (its conditions,
    /// filter chips and search) and the Dio's residual still apply.
    ///
    /// - A paged view whose master takes the whole query counts at the
    ///   master. One that leaves a clause over counts the complete set its
    ///   Lens copies through an eager start, and without one has no count.
    /// - An eager view counts its cache once it has settled.
    /// - A two-pass view has no count.
    ///
    /// `Ok(None)` means no honest count. The future is runtime-agnostic: it
    /// may be awaited from any executor, and dropping it abandons the count.
    fn count_where(
        &self,
        _terms: Vec<OpCondition>,
    ) -> futures::future::BoxFuture<'static, vantage_core::Result<Option<usize>>> {
        Box::pin(async { Ok(None) })
    }

    /// Whether [`Self::set_filter_terms`] with `terms` leaves the view able
    /// to answer: a paged view needs a master that takes every term, or a
    /// Lens that copies the whole set when it doesn't; an eager or two-pass
    /// view evaluates them over its own rows, unless a titles-only picker
    /// would have to judge an augmented column it never hydrates.
    fn terms_supported(&self, _terms: &[OpCondition]) -> bool {
        false
    }

    /// Replace the quicksearch text (replace semantics; `None` or blank
    /// clears). Paged sceneries push it into every chunk fetch — callers
    /// should offer search only when [`Self::search_supported`] says so.
    /// Eager sceneries filter locally over their (complete) cache. Default
    /// no-op for wrappers that don't search.
    fn set_search(&self, _query: Option<String>) {}

    /// Whether [`Self::set_search`] will do something honest here: a paged
    /// scenery needs the master's `Search` (a local filter would silently
    /// search only the loaded slice); an eager one always can.
    fn search_supported(&self) -> bool {
        false
    }

    fn subscribe(&self) -> watch::Receiver<Generation>;

    /// The view's current [`ViewStats`]: totals, loaded and visible rows,
    /// order, search, filters and live state. Cheap — a clone of the last
    /// published snapshot, never a fetch.
    fn view_stats(&self) -> ViewStats {
        ViewStats::from_scenery(self)
    }

    /// Signals each time [`view_stats`](Self::view_stats) changes. Separate
    /// from [`subscribe`](Self::subscribe): a viewport move or a fetch
    /// starting changes the snapshot without changing any row, and a row
    /// hydrating changes a row without changing the snapshot.
    fn subscribe_view_stats(&self) -> watch::Receiver<Generation> {
        self.subscribe()
    }

    /// Snapshot of the master Vista's capability flags taken at open
    /// time. UI delegates branch on these to pick the right page
    /// primitive: `Window` → drive everything through
    /// `set_viewport`; cursor-only (`Cursor`) → call
    /// `request_load_more` to walk forward.
    fn master_capabilities(&self) -> &VistaCapabilities;

    /// The columns this view declared it shows (its demand, from the
    /// builder's `columns()`). `None` = demands everything — the default for
    /// implementations that don't track demand.
    fn demanded_columns(&self) -> Option<Vec<String>> {
        None
    }
}
