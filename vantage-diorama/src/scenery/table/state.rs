use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex, RwLock};

use ciborium::Value as CborValue;
use tokio::sync::{Notify, mpsc, watch};
use vantage_vista::VistaCapabilities;

use crate::dio::{DioInner, Generation};

use super::{SortDir, ViewportRequest};

mod chunk_target;
mod load_state;
mod query;
mod reseed;
mod rows;
mod totals;

/// Internal state shared by the public scenery handle, the reactor
/// task, and the viewport-debounce task.
pub(crate) struct TableSceneryState {
    /// Live-instance census (see [`crate::stats`]).
    pub(crate) _tally: crate::stats::Tally,
    /// Weak so the Scenery doesn't pin the Dio alive — the spawned
    /// tasks exit when the last user-held Dio drops.
    pub(crate) dio_weak: std::sync::Weak<DioInner>,
    /// A sugared wrapper this view reads rows through, when opened with
    /// [`sugar`](super::TableSceneryBuilder::sugar). See [`Self::reader`].
    pub(crate) read_cache: Option<Arc<dyn crate::lens::cache_backend::CacheTable>>,

    pub(crate) conditions: RwLock<Vec<(String, CborValue)>>,
    /// Non-equality filters applied locally over the cache (see
    /// [`OpCondition`](super::OpCondition)). Separate from `conditions` so the
    /// eq path is unchanged; both are ANDed in the reseed filter chain.
    pub(crate) op_conditions: RwLock<Vec<super::OpCondition>>,
    pub(crate) sort: RwLock<Option<(String, SortDir)>>,

    pub(crate) rows: RwLock<super::row_map::RowMap>,
    pub(crate) id_to_idx: RwLock<HashMap<String, usize>>,
    pub(crate) total: RwLock<Option<usize>>,
    /// How far `total` can be trusted — see [`TotalKind`](super::view_stats::TotalKind).
    pub(crate) total_kind: RwLock<super::view_stats::TotalKind>,
    /// The published [`ViewStats`](super::ViewStats) snapshot.
    pub(crate) stats: super::view_stats::StatsCell,

    /// The most recent viewport range handed to the loader. A refresh on a
    /// chunk-loaded scenery re-fetches exactly this range in place (see
    /// [`refresh_loaded_viewport`](Self::refresh_loaded_viewport)). `None`
    /// until the first viewport is set.
    pub(crate) last_viewport: RwLock<Option<Range<usize>>>,

    pub(crate) page_size: usize,

    pub(crate) generation: AtomicU64,
    pub(crate) generation_tx: watch::Sender<Generation>,

    pub(crate) reload_notify: Arc<Notify>,
    pub(crate) viewport_tx: mpsc::UnboundedSender<ViewportRequest>,

    /// Mirrors the live depth of `viewport_tx`. Bumped on every send,
    /// decremented every time the loader pops a message. Surfaces the
    /// backlog when chunk fetches can't keep up with scroll bursts.
    pub(crate) viewport_queue_depth: AtomicUsize,

    /// True while a chunk load is currently dispatched — prevents
    /// `request_load_more` from queueing the same range twice in a row.
    pub(crate) load_in_flight: Mutex<Option<Range<usize>>>,

    /// Set by [`write_chunk_row`](Self::write_chunk_row) whenever a chunk load
    /// actually changes a row's visible content (new slot, status change, or a
    /// different record). The loader reads and clears it after the load and
    /// bumps the generation only when it is set — so a refresh that re-fetches
    /// byte-identical rows does not signal a repaint.
    pub(crate) load_dirty: std::sync::atomic::AtomicBool,

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
    pub(crate) load_total_reported: std::sync::atomic::AtomicBool,

    /// Whether ANY fetch has ever stated a grand total (as opposed to totals
    /// this side inferred from short pages). Latched, never cleared: a source
    /// either reports totals or it doesn't. While it never has, a full page
    /// reaching the advertised end means "horizon", not "end" — the loader
    /// extends the addressable set so scrolling can keep asking (the
    /// grows-as-you-scroll mode for window-paged, total-less sources).
    pub(crate) total_ever_stated: std::sync::atomic::AtomicBool,

    /// Whether this view has finished its first load.
    ///
    /// "No rows" and "no rows *yet*" are the same picture and opposite
    /// meanings: a grid that paints its empty state while the first fetch is
    /// still in flight tells the user the table is empty, and they believe it.
    /// Set once, by whichever path first puts rows in front of the user —
    /// a seed from cache, a chunk load, or the two-pass list — and never
    /// cleared, because a later refresh returning nothing is a real answer.
    pub(crate) settled: std::sync::atomic::AtomicBool,

    /// Whether this view loads a window at a time (rather than over a cache the
    /// lens filled whole). Decided once at open from the lens's offers and the
    /// master's capabilities — see `builder::pages_lazily`.
    ///
    /// Recorded rather than re-derived because the two consumers must not be
    /// able to disagree. The loader used to infer it from "is an `on_load_chunk`
    /// registered", which was the same answer only while every table had its own
    /// lens. Sharing one lens across a datasource made that callback always
    /// present, and an eager table's viewport then asked a source for a window
    /// it had never claimed to serve.
    pub(crate) paged: bool,

    /// Snapshot of the master Vista's capability flags taken at open
    /// time. Sceneries hand this back through
    /// `TableScenery::master_capabilities` so UI delegates can route
    /// page requests through the right primitive (`set_viewport` for
    /// `can_fetch_page`, `request_load_more` for `can_fetch_next`).
    pub(crate) master_capabilities: VistaCapabilities,

    // ---- two-pass loading -------------------------------------------------
    //
    // Populated only when the Lens registers an `on_load_detail` callback.
    // `two_pass == false` leaves every field below inert and the scenery on
    // the legacy single-pass path.
    /// Whether this scenery drives two-pass (list + detail) loading.
    pub(crate) two_pass: bool,
    /// UI-level equality filters (grid filter chips) — runtime-toggled,
    /// ANDed with `conditions` in the local refine chain, kept separate
    /// so chips can never clobber query narrowing.
    pub(crate) ui_filters: RwLock<Vec<(String, CborValue)>>,

    /// UI-level operator filters (the grid's filter panel) — runtime-set
    /// `column <op> value` terms. They follow quicksearch's mechanics rather
    /// than `op_conditions`' build-time ones: a paged scenery carries them
    /// into every chunk fetch and the master pushes what it can, an eager
    /// one evaluates them over its complete cache.
    pub(crate) ui_terms: RwLock<Vec<super::OpCondition>>,

    /// Active quicksearch text (`None` when not searching). Paged sceneries
    /// carry it into every chunk fetch (`ChunkQuery`); eager ones apply it as
    /// a local predicate in `reseed_from_cache` — honest there, because the
    /// cache is (or becomes) the complete set.
    pub(crate) search: RwLock<Option<String>>,
    /// Dropdown / autocomplete projection: serve the cheap list columns and
    /// **skip the detail pass** even on a two-pass table. The list pass still
    /// runs (rows carry id + title columns); per-row hydration never fires.
    pub(crate) titles_only: bool,
    /// The columns this view declared it shows (its **demand**), from the
    /// builder's `columns()`. `None` = demands everything. The Dio unions the
    /// demands of its open sceneries to gate the augment detail pass — see
    /// [`DioInner::demanded_columns`](crate::dio::DioInner::demanded_columns).
    pub(crate) demand: Option<Vec<String>>,
    /// The shared per-query ordered index for this scenery's conditions/sort,
    /// keyed by [`Vista::index_key`](vantage_vista::Vista::index_key). `None` in single-pass mode.
    /// Swappable: a `set_sort` re-points it at the index for the
    /// new variant (see [`resort`](super::two_pass::resort)).
    pub(crate) index: RwLock<Option<Arc<crate::dio::query_index::QueryIndex>>>,
    /// This scenery's key in the Dio's dedup registry, captured at open. Cleared
    /// (and the registry entry removed) the first time the handle mutates its own
    /// query in place — a bespoke, resorted scenery is no longer the shareable
    /// canonical one, so a later open under the old key must not get it back.
    pub(crate) registry_key: Mutex<Option<String>>,
    /// This scenery's requester handle into the Dio's central augment
    /// scheduler (two-pass only). The detail pass enqueues ids here; the
    /// ticket's drop — with the last handle to this state — withdraws
    /// anything still queued, so a closing view stops pulling.
    pub(crate) augment_ticket: Option<crate::dio::augment_scheduler::AugmentTicket>,
    /// True while a list-page fetch is dispatched, so overlapping
    /// `request_load_more` calls don't double-page.
    pub(crate) list_in_flight: Mutex<bool>,

    // ---- debug stream -------------------------------------------------
    //
    /// This scenery's Dio's debug tap, cloned at open. `note_state` needs it
    /// from [`mark_settled`](Self::mark_settled), which has no `DioInner` (it
    /// runs deep inside the loader after the Dio has already been consulted) —
    /// carrying the tap here means `note_state` needs no other path to reach it.
    pub(crate) debug_tap: crate::debug::DebugTap,
    /// The Dio's master name at open, for the same reason `debug_tap` is
    /// carried here rather than reached through `dio_weak`.
    pub(crate) dio_name: String,
    /// The [`LoadState`](super::LoadState) last emitted by
    /// [`note_state`](Self::note_state), so a repeat call (settling, then the
    /// same load's generation bump) doesn't double-log an unchanged state.
    pub(crate) last_debug_state: Mutex<Option<super::LoadState>>,
    /// Whether the debug stream has already named this view's columns. The
    /// set doesn't change between fetches, so it's said once.
    pub(crate) payload_named: std::sync::atomic::AtomicBool,
    /// The last range reported as served from cache. A view that re-declares
    /// the same viewport (a repaint, a focus change) would otherwise log the
    /// same "served locally" line twice in a row, saying nothing new.
    pub(crate) last_served: Mutex<Option<std::ops::Range<usize>>>,
}

/// Clears `load_in_flight` when dropped, so a load that is cancelled
/// mid-flight — its future dropped by the viewport loop — cannot leave its
/// range marked as still loading and make the next request for that range
/// skip itself. Owns the `Arc` rather than borrowing it so it can travel
/// inside `PendingChunk` from `run_chunk_callback` to `finish_chunk_load`,
/// holding the guard for the whole load rather than just its network half.
pub(crate) struct InFlightMarker(pub(crate) Arc<TableSceneryState>);

impl Drop for InFlightMarker {
    fn drop(&mut self) {
        *self.0.load_in_flight.lock().unwrap() = None;
        self.0.publish_view_stats();
    }
}
