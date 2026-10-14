use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex, RwLock, Weak};

use tokio::sync::{Notify, mpsc};
use vantage_vista::{Condition, VistaCapabilities, VistaQuery};

use crate::dio::DioInner;
use crate::dio::augment_scheduler::AugmentTicket;
use crate::lens::cache_backend::CacheTable;

use super::ViewportRequest;
use super::view::{ViewHook, ViewId};

mod chunk_target;
mod copy;
mod guards;
mod load_state;
mod query;
mod reseed;
mod rows;
mod splice;
mod totals;

pub(crate) use copy::Init;
pub(crate) use guards::{InFlightMarker, TaskGuard};

/// The rows answering one query, shared by every view of that query: the
/// visible map, the total, the reactor and the viewport loop. The query is
/// fixed at creation; a view whose query changes moves to other
/// `SharedRows` (see `view::repoint`) and leaves these untouched.
pub(crate) struct SharedRows {
    /// Weak so the rows don't pin the Dio alive — the spawned tasks exit
    /// when the last user-held Dio drops.
    pub(crate) dio_weak: Weak<DioInner>,
    /// The registry key, [`shared_key`](crate::dio::shared_key) of `query`.
    /// Rows a sugared view reads are never registered, but still carry it.
    pub(crate) key: String,
    /// A sugared wrapper these rows are read through, when the view that
    /// built them was opened with [`sugar`](super::TableSceneryBuilder::sugar).
    /// See [`Self::reader`].
    pub(crate) read_cache: Option<Arc<dyn CacheTable>>,
    /// The sugar's output columns, empty without sugar. A clause on one is
    /// never offered to the master.
    pub(crate) sugar_columns: Arc<HashSet<String>>,

    /// Every clause these rows answer: the builder's filters, the runtime
    /// filters, search and order.
    pub(crate) query: VistaQuery,
    /// The builder filters of the view that built these rows. With the order
    /// they key the two-pass index; runtime filters are refined locally.
    pub(crate) base: Vec<Condition>,

    pub(crate) rows: RwLock<super::row_map::RowMap>,
    pub(crate) id_to_idx: RwLock<HashMap<String, usize>>,
    /// The id at each slot, the reverse of `id_to_idx`, kept for
    /// [`splice_one`](Self::splice_one). `None` once anything else writes
    /// `rows` or `id_to_idx`; the next splice rebuilds it. Taken after both.
    pub(crate) idx_to_id: Mutex<Option<Vec<String>>>,
    pub(crate) total: RwLock<Option<usize>>,
    /// How far `total` can be trusted — see [`TotalKind`](super::view_stats::TotalKind).
    pub(crate) total_kind: RwLock<super::view_stats::TotalKind>,
    /// The published [`ViewStats`](super::ViewStats) snapshot, before each
    /// view lays its own shown range and filters over it.
    pub(crate) stats: super::view_stats::StatsCell,

    /// The views reading these rows, with their viewports. Bumps fan out to
    /// them, a refresh re-fetches their viewports, and the rows live as long
    /// as one of them holds them.
    pub(crate) views: Mutex<HashMap<ViewId, ViewHook>>,

    /// The page size of the view that built these rows: one list page, the
    /// on-open fetch, and how far a fetch grows past a viewport.
    pub(crate) page_size: usize,

    pub(crate) reload_notify: Arc<Notify>,
    pub(crate) viewport_tx: mpsc::UnboundedSender<ViewportRequest>,

    /// Mirrors the live depth of `viewport_tx`. Bumped on every send,
    /// decremented every time the loader pops a message. Surfaces the
    /// backlog when chunk fetches can't keep up with scroll bursts.
    pub(crate) viewport_queue_depth: AtomicUsize,

    /// True while a chunk load is currently dispatched — prevents
    /// `request_load_more` from queueing the same range twice in a row.
    pub(crate) load_in_flight: Mutex<Option<Range<usize>>>,

    /// Set by [`write_chunk_row`](crate::lens::chunk_sink::SceneryChunkTarget::write_chunk_row) whenever a chunk load
    /// actually changes a row's visible content (new slot, status change, or a
    /// different record). The loader reads and clears it after the load and
    /// bumps the generation only when it is set — so a refresh that re-fetches
    /// byte-identical rows does not signal a repaint.
    pub(crate) load_dirty: AtomicBool,

    /// Count of rows the in-flight chunk load *received* (every push, including
    /// those `write_chunk_row` skips as unchanged). A short page — fewer rows
    /// than the requested window — means the end of the set, so the loader
    /// derives the grand `total` from it (no separate count request).
    pub(crate) load_push_count: AtomicUsize,

    /// Whether the in-flight chunk load reported a grand total of its own (via
    /// [`ChunkSink::set_total`](crate::ChunkSink::set_total)).
    ///
    /// It settles which of two answers wins. A short page is only *evidence* of
    /// the end of the set; a total the source stated in the same response is
    /// the fact. They disagree whenever a source caps its page below what we
    /// asked for — 25 rows back from a request for 100 means "here is a page",
    /// not "there are 25 rows" — and inferring from the page there would cut
    /// the grid off at its first screen.
    pub(crate) load_total_reported: AtomicBool,

    /// Whether ANY fetch has ever stated a grand total (as opposed to totals
    /// this side inferred from short pages). Latched, never cleared: a source
    /// either reports totals or it doesn't. While it never has, a full page
    /// reaching the advertised end means "horizon", not "end" — the loader
    /// extends the addressable set so scrolling can keep asking (the
    /// grows-as-you-scroll mode for window-paged, total-less sources).
    pub(crate) total_ever_stated: AtomicBool,

    /// Whether these rows have finished their first load.
    ///
    /// "No rows" and "no rows *yet*" are the same picture and opposite
    /// meanings: a grid that paints its empty state while the first fetch is
    /// still in flight tells the user the table is empty, and they believe it.
    /// Set by whichever path first puts rows in front of the user — a seed
    /// from cache, a chunk load, or the two-pass list. A later refresh
    /// returning nothing is a real answer, so only a change of
    /// [`serving`](Self::serving), which drops the rows, clears it.
    pub(crate) settled: AtomicBool,

    /// Whether the map is this query's whole answer in its order, as a
    /// reseed from the cache installs it. Copied rows start without it, so a
    /// changed row is spliced in only once the rows match their query.
    pub(crate) placed: AtomicBool,

    /// Whether the visible map leaves out a listed row its query couldn't
    /// judge, because the row's detail fetch failed. The view then reads
    /// `Partial`: its set is short by those rows.
    pub(crate) omits_failed_rows: AtomicBool,

    /// Whether these rows load a window at a time (rather than over a cache
    /// the lens filled whole). Decided at open from the lens's offers and the
    /// master's capabilities — see `builder::open::pages_lazily`.
    ///
    /// Recorded rather than re-derived because the two consumers must not be
    /// able to disagree. Inferring it from "does the Lens have a paged loader"
    /// gives the same answer only while every table has its own lens. Sharing
    /// one lens across a datasource makes that loader always present, and an
    /// eager table's viewport then asks a source for a window it never claimed
    /// to serve.
    pub(crate) paged: bool,

    /// Snapshot of the master Vista's capability flags taken at open time.
    pub(crate) master_capabilities: VistaCapabilities,

    // ---- two-pass loading -------------------------------------------------
    //
    // Populated only when the Dio owns augmentation.
    // `two_pass == false` leaves every field below inert and the rows on the
    // single-pass path.
    /// Whether these rows drive two-pass (list + detail) loading.
    pub(crate) two_pass: bool,
    /// The Dio's plan for `query`. See [`ViewPlan`](super::plan::ViewPlan).
    pub(crate) view_plan: RwLock<Option<Arc<super::plan::ViewPlan>>>,
    /// Where the answer to `query` comes from. See
    /// [`Serving`](super::plan::Serving).
    pub(crate) serving: RwLock<super::plan::Serving>,
    /// Dropdown / autocomplete projection: serve the cheap list columns and
    /// **skip the detail pass** even on a two-pass table. The list pass still
    /// runs (rows carry id + title columns); per-row hydration never fires.
    pub(crate) titles_only: bool,
    /// The shared per-query ordered index for `base` and the order, keyed by
    /// `query_index::index_key`. `None` in single-pass mode.
    pub(crate) index: RwLock<Option<Arc<crate::dio::query_index::QueryIndex>>>,
    /// The requester handle whole-index sweeps hydrate under (two-pass only):
    /// a view refining on an augmented value hydrates every listed row, not
    /// one view's viewport. A view's own viewport hydrates under the view's
    /// ticket instead. Dropping it with the rows withdraws what is queued.
    pub(crate) sweep_ticket: Option<Arc<AugmentTicket>>,
    /// True while a list-page fetch is dispatched, so overlapping
    /// `request_load_more` calls don't double-page.
    pub(crate) list_in_flight: Mutex<bool>,
    /// The batched reseed a moved row asks for on a refined view.
    pub(crate) coalescer: Arc<super::two_pass::Coalescer>,
    /// Held across one refined reseed, read to install. Reseeds start from
    /// the reactor, the list pass and the coalescer; one that read before an
    /// index change must not install after the reseed that follows it.
    pub(crate) reseed_lock: tokio::sync::Mutex<()>,

    // ---- debug stream -------------------------------------------------
    //
    /// The Dio's debug tap, cloned at creation. `note_state` needs it from
    /// [`mark_settled`](Self::mark_settled), which has no `DioInner`.
    pub(crate) debug_tap: crate::debug::DebugTap,
    /// The [`LoadState`](super::LoadState) last emitted by
    /// [`note_state`](Self::note_state), so a repeat call (settling, then the
    /// same load's generation bump) doesn't double-log an unchanged state.
    pub(crate) last_debug_state: Mutex<Option<super::LoadState>>,
    /// Whether the debug stream has already named these rows' columns. The
    /// set doesn't change between fetches, so it's said once.
    pub(crate) payload_named: AtomicBool,
    /// The last range reported as served from cache. A view that re-declares
    /// the same viewport (a repaint, a focus change) would otherwise log the
    /// same "served locally" line twice in a row, saying nothing new.
    pub(crate) last_served: Mutex<Option<Range<usize>>>,

    /// The reactor, the viewport loop and any detached list pass. Aborted
    /// the moment the last view detaches (see [`Self::detach`]), even while
    /// one of them holds the rows for a load, so a closing grid stops
    /// pulling; and again when the rows drop.
    pub(crate) tasks: TaskGuard,
}

impl SharedRows {
    /// Attach `view` while some other view still reads these rows. Rows
    /// whose last view left are closed for good — their work was aborted —
    /// so the hook comes back and the caller builds fresh rows. Checked and
    /// inserted under the `views` lock that [`detach`](Self::detach) takes.
    pub(crate) fn try_attach(&self, view: ViewId, hook: ViewHook) -> Result<(), ViewHook> {
        let mut views = self.views.lock().unwrap_or_else(|e| e.into_inner());
        if views.is_empty() {
            return Err(hook);
        }
        views.insert(view, hook);
        Ok(())
    }

    /// Detach `view` and hand back its hook. When it was the last view, the
    /// rows' work is aborted under the same lock, so no load they started
    /// reaches the master afterwards and nobody attaches meanwhile.
    pub(crate) fn detach(&self, view: ViewId) -> Option<ViewHook> {
        let mut views = self.views.lock().unwrap_or_else(|e| e.into_inner());
        let hook = views.remove(&view);
        if views.is_empty() {
            self.tasks.abort_all();
        }
        hook
    }
}
