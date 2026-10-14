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
mod cap;
mod count;
mod handle;
mod load_failure;
mod loader;
mod plan;
mod reactor;
mod row_map;
mod state;
mod two_pass;
mod view;
mod view_stats;

use std::ops::Range;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::watch;
use vantage_core::Result;
use vantage_vista::{Condition, SortDirection, VistaCapabilities};

use crate::dio::Generation;

use super::enriched_record::EnrichedRecord;

pub use builder::TableSceneryBuilder;
pub(crate) use handle::compute_load_state;
pub use load_failure::{Clause, LoadFailure};
pub(crate) use state::SharedRows;
pub(crate) use view::TableView;
pub use view_stats::{FilterOrigin, ViewCount, ViewFilter, ViewState, ViewStats};

/// Internal viewport request carried over the debounce channel.
#[derive(Debug, Clone)]
pub(crate) struct ViewportRequest {
    /// The view that asked, or `None` for a request the rows make for
    /// themselves. The viewport loop coalesces each view's requests apart,
    /// and a two-pass request hydrates under its view's ticket.
    pub(crate) view: Option<view::ViewId>,
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
#[non_exhaustive]
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
///
/// Every method is required: the one implementation is the view
/// [`TableSceneryBuilder::open`] returns, and a test double states each
/// answer rather than inherit a guess.
pub trait TableScenery: Send + Sync {
    fn row_count(&self) -> usize;

    /// How much of this view's data has arrived.
    ///
    /// Three states, because two are not enough to act on: a consumer needs to
    /// tell "no rows" from "no rows *yet*" (they render identically and mean
    /// opposite things), and a test or a profiler needs to tell "showing
    /// something" from "showing everything" before it measures.
    fn load_state(&self) -> LoadState;

    /// Status breakdown over the rows currently in the sparse map. Used by the
    /// diagnostics surface to report how much of a scenery is hydrated.
    fn status_summary(&self) -> RowStatusSummary;
    fn has_more(&self) -> bool;
    fn estimated_total(&self) -> Option<usize>;
    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>>;

    /// The row with id `id`, wherever the view's order puts it; `None` when
    /// the view doesn't hold it (not loaded, filtered out, or beyond a
    /// [`limit`](TableSceneryBuilder::limit) or [`tail`](TableSceneryBuilder::tail)).
    fn row_by_id(&self, id: &str) -> Option<Arc<EnrichedRecord>>;

    /// Declare the rows to load and hydrate (`loaded`) and the rows a
    /// displaying consumer has on screen (`shown`), both in the view's
    /// indices. An empty `shown` means none on screen: a dashboard counter
    /// or an observation probe loads rows without claiming the status bar's
    /// "Showing a–b". `shown` is the only input to [`ViewStats::showing`].
    /// A grid typically passes the same range twice.
    fn set_viewport(&self, loaded: Range<usize>, shown: Range<usize>);

    fn request_load_more(&self);

    /// Ask the Dio to refresh from its source; the view reloads as the
    /// fresh rows land.
    fn refresh_source(&self);

    /// Replace the view's order; `None` clears it, leaving the Dio's order
    /// or none. A [`tail`](TableSceneryBuilder::tail) needs an order:
    /// clearing it fails the view until one is set again.
    fn set_order(&self, order: Option<(String, SortDirection)>);

    /// Replace the runtime filters (the grid's chips and filter panel):
    /// conditions ANDed together and with the builder's
    /// [`filter`](TableSceneryBuilder::filter)s. Same mechanics as
    /// [`set_search`](Self::set_search): a paged scenery carries them into
    /// every chunk fetch and the master takes what it can, handing the rest
    /// back as the view's residual; an eager scenery evaluates them over its
    /// complete cache (which is what lets them name augmented columns). An
    /// empty vec clears. The current set reads back from
    /// [`ViewStats::filters`].
    fn set_filters(&self, filters: Vec<Condition>);

    /// How many rows this view would hold with `filters` in place of its
    /// current runtime filters. The builder's filters, the search and the
    /// Dio's residual still apply.
    ///
    /// - A paged view whose master takes the whole query counts at the
    ///   master. One that leaves a clause over counts the complete set its
    ///   Lens copies through an eager start, and without one has no count.
    /// - An eager view counts its cache once it has settled.
    /// - A two-pass view has no count.
    ///
    /// `Ok(None)` means no honest count. The future is runtime-agnostic: it
    /// may be awaited from any executor, and dropping it abandons the count.
    fn count_where(&self, filters: Vec<Condition>) -> BoxFuture<'static, Result<Option<usize>>>;

    /// Whether [`Self::set_filters`] with `filters` leaves the view able to
    /// answer: a paged view needs a master that takes every filter, or a
    /// Lens that copies the whole set when it doesn't; an eager or two-pass
    /// view evaluates them over its own rows, unless a titles-only picker
    /// would have to judge an augmented column it never hydrates.
    fn filters_supported(&self, filters: &[Condition]) -> bool;

    /// Replace the quicksearch text (replace semantics; `None` or blank
    /// clears). Paged sceneries push it into every chunk fetch — callers
    /// should offer search only when [`Self::search_supported`] says so.
    /// Eager sceneries filter locally over their (complete) cache.
    fn set_search(&self, text: Option<String>);

    /// Whether [`Self::set_search`] will do something honest here, decided
    /// for the view's current query as [`Self::filters_supported`] decides:
    /// a paged view needs a master that searches, or a Lens that copies the
    /// whole set when it doesn't (a local filter over a window would search
    /// only the loaded slice); an eager view always can.
    fn search_supported(&self) -> bool;

    fn subscribe(&self) -> watch::Receiver<Generation>;

    /// The view's current [`ViewStats`]: totals, loaded and visible rows,
    /// order, search, filters and live state. Cheap — a clone of the last
    /// published snapshot, never a fetch.
    fn view_stats(&self) -> ViewStats;

    /// Signals each time [`view_stats`](Self::view_stats) changes. Separate
    /// from [`subscribe`](Self::subscribe): a viewport move or a fetch
    /// starting changes the snapshot without changing any row, and a row
    /// hydrating changes a row without changing the snapshot.
    fn subscribe_view_stats(&self) -> watch::Receiver<Generation>;

    /// Snapshot of the master Vista's capability flags taken at open
    /// time. UI delegates branch on these to pick the right page
    /// primitive: `Window` → drive everything through
    /// `set_viewport`; cursor-only (`Cursor`) → call
    /// `request_load_more` to walk forward.
    fn master_capabilities(&self) -> &VistaCapabilities;
}
