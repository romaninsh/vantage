//! The state every clone of a [`Dio`](super::Dio) shares, and the helpers the
//! rest of the crate reaches it through.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use vantage_vista::{Vista, VistaMetadata, VistaQuery};

use super::{
    AbortOnDrop, DetailState, DioEvent, WriteCapabilities, augment_scheduler, pending, query_index,
    worker,
};
use crate::augment::Augmentation;
use crate::lens::{CacheTable, Lens};
use crate::scenery::table::{SharedRows, TableView};

mod sceneries;
mod shared_rows;

pub(crate) use shared_rows::shared_key;

pub(crate) struct DioInner {
    /// Live-instance census (see [`crate::stats`]).
    pub(crate) _tally: crate::stats::Tally,
    pub(crate) lens: Arc<Lens>,
    /// The master Vista, narrowed by every clause it accepted.
    pub(crate) master: Vista,
    /// The clauses the master refused, or never saw because they name an
    /// augmented column. Evaluated by diorama over the rows it holds.
    pub(crate) residual: VistaQuery,
    /// The master's applied query with the residual laid over it: everything
    /// this Dio is narrowed by. Its key is the cache identity.
    pub(crate) query: VistaQuery,
    /// The master's schema, for evaluating `query` over a row.
    pub(crate) metadata: VistaMetadata,
    /// The master's id column, resolved once when the Dio was built.
    pub(crate) id_column: String,
    /// Whether the Dio follows the master's change stream.
    pub(crate) live: bool,
    pub(crate) cache: Arc<dyn CacheTable>,
    pub(crate) cache_table_name: String,
    pub(crate) write_queue: mpsc::Sender<worker::QueuedFlash>,
    pub(crate) event_bus: broadcast::Sender<DioEvent>,
    /// Rows with an optimistic flash in flight — reconcile-shaped cache
    /// writers skip them so a stale master snapshot can't clobber a
    /// staged value. See [`pending::PendingFlashes`].
    pub(crate) pending_flashes: Arc<pending::PendingFlashes>,
    pub(crate) refresh_task: Mutex<Option<JoinHandle<()>>>,
    pub(crate) write_worker: Mutex<Option<JoinHandle<()>>>,
    /// Per-query ordered indexes, keyed by [`query_index::index_key`]. Shared across
    /// every two-pass scenery of this Dio so reopening the same filter/sort
    /// reuses the already-built index. Not persisted — re-listing rebuilds it.
    pub(crate) query_indexes: std::sync::Mutex<HashMap<String, Arc<query_index::QueryIndex>>>,
    /// The augmentations fixed when the Dio was built. When set, the Dio drives
    /// its own two-pass list/detail/refresh from them (see `augment_passes`).
    /// A Dio derived from this one shares the same `Arc`.
    pub(crate) augmentations: Option<Arc<Vec<Augmentation>>>,
    /// The union of every augmentation's merged columns: client-side columns
    /// the master has never heard of.
    pub(crate) augmented_columns: HashSet<String>,
    /// The rows every live table view reads, keyed by
    /// [`shared_key`](shared_rows::shared_key): views of one query share one
    /// set of rows (one reactor, one fetch per window). `Weak`, so the rows
    /// drop, and their tasks abort, with the last view pointing at them.
    pub(crate) shared_rows: std::sync::Mutex<HashMap<String, std::sync::Weak<SharedRows>>>,
    /// Every open table view, for the demand union, the census and the
    /// diagnostics. `Weak`; closed views are pruned on the next open.
    pub(crate) table_views: std::sync::Mutex<Vec<std::sync::Weak<TableView>>>,
    /// Central per-row detail-fetch scheduler: every consumer (scenery
    /// viewport, blocking facade read) queues ids here and a small worker
    /// pool drains them — one flight per row, round-robin across consumers.
    /// Inert (no tasks) until [`ensure_augment_workers`](Self::ensure_augment_workers) runs.
    pub(crate) augment_scheduler: Arc<augment_scheduler::AugmentScheduler>,
    /// Worker tasks spawned by `ensure_augment_workers`, aborted when the
    /// Dio drops — a parked worker holds only a `Weak` and would otherwise
    /// idle forever.
    pub(crate) augment_worker_handles: std::sync::Mutex<Vec<JoinHandle<()>>>,
    /// The detail pass's last answer per row, one per id. See
    /// [`detail_state`](super::detail_state).
    pub(crate) detail_state: std::sync::Mutex<HashMap<String, DetailState>>,

    /// False only while a detached eager start is still copying the set into
    /// the cache. True once it finishes — success or failure alike, since a
    /// failed seed means no rows are coming either.
    ///
    /// A scenery reads this to tell an empty cache apart from an empty table.
    /// The two look identical from the cache alone, and calling the first one
    /// "no rows" is what makes a grid flash its empty state a moment before its
    /// rows land. True from construction when there is nothing to wait for:
    /// no eager start, or a blocking one that has already run.
    pub(crate) seed_complete: std::sync::atomic::AtomicBool,
    /// The whole copy a paged view asks for when its query leaves a clause
    /// the master can't apply. `None` until one is asked for. See
    /// [`copy_whole`](Self::copy_whole).
    pub(crate) whole_copy:
        std::sync::Mutex<Option<tokio::sync::watch::Receiver<super::whole_copy::CopyOutcome>>>,
    /// When this Dio was built. With no `max_age` to bound it, a whole copy
    /// recorded before this is an earlier session's and isn't trusted (see
    /// [`holds_whole_copy`](Self::holds_whole_copy)).
    pub(crate) built_at: std::time::SystemTime,
    /// Set while a `max_age` check (and the refresh it may request) is in
    /// flight, so a burst of scenery opens checks once.
    pub(crate) stale_check_pending: std::sync::atomic::AtomicBool,

    /// Monotonic per-dio request id for correlating debug-stream
    /// dispatch/return lines (`req=N`).
    pub(crate) req_seq: std::sync::atomic::AtomicU64,
    /// Live servos opened on this Dio — census bookkeeping only.
    pub(crate) servo_census: std::sync::atomic::AtomicUsize,
    /// Live record sceneries opened on this Dio — census bookkeeping only.
    pub(crate) record_census: std::sync::atomic::AtomicUsize,
    /// Never sent on; dropping it with the Dio is the signal. The live-follow
    /// task holds a receiver and returns as soon as it closes, so a quiet
    /// stream doesn't outlive its Dio.
    pub(crate) watch_cancel: tokio::sync::watch::Sender<()>,
    /// A task that exists only to serve this Dio, such as the recomputation
    /// feeding a derived aggregate. It ends when the last holder drops.
    pub(crate) owned_task: OnceLock<AbortOnDrop>,
}

impl Drop for DioInner {
    fn drop(&mut self) {
        for handle in self.augment_worker_handles.lock().unwrap().drain(..) {
            handle.abort();
        }
    }
}

impl DioInner {
    /// The debug tap this Dio's Lens carries — the source of truth every
    /// `tapline!` call site reaches through.
    pub(crate) fn tap(&self) -> &crate::debug::DebugTap {
        &self.lens.debug
    }

    /// Next value in this Dio's request sequence — stamped as `req=N` to
    /// correlate a dispatch line with its return line in the debug stream.
    pub(crate) fn next_req(&self) -> u64 {
        // 1-based: "fetch #1" is the first request a reader sees, and a
        // zero-th of anything reads like an off-by-one bug in the log.
        self.req_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    /// See [`Dio::write_capabilities`](super::Dio::write_capabilities). Lives on
    /// the inner so [`DioShell`](super::DioShell) can share the one definition of
    /// capability lifting.
    pub(crate) fn write_capabilities(&self) -> WriteCapabilities {
        use vantage_vista::Capability;
        let routed = self.lens.callbacks.writer.is_some();
        let caps = self.master.capabilities();
        WriteCapabilities {
            can_insert: caps.has(Capability::Insert) || routed,
            can_update: caps.has(Capability::Update) || routed,
            can_delete: caps.has(Capability::Delete) || routed,
        }
    }

    /// Fetch (or lazily create) the [`QueryIndex`](query_index::QueryIndex)
    /// for `key`. Repeated calls with the same key return the same `Arc`, so
    /// all sceneries on a query variant share one ordered index.
    pub(crate) fn query_index(&self, key: &str) -> Arc<query_index::QueryIndex> {
        let mut guard = self.query_indexes.lock().unwrap();
        guard
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(query_index::QueryIndex::new()))
            .clone()
    }

    /// Whether this Dio owns an augmentation config, which is what makes its
    /// table sceneries two-pass.
    pub(crate) fn has_dio_augment(&self) -> bool {
        self.augmentations.is_some()
    }

    /// Whether `col` is produced by augmentation: a client-side column the
    /// master can't filter or sort on.
    pub(crate) fn is_augmented_column(&self, col: &str) -> bool {
        self.augmented_columns.contains(col)
    }
}
