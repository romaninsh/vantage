//! `LiveTable` — write-through cache wrapper around an `AnyTable` master.
//!
//! Reads consult the cache first, fall through to the master on a miss, and
//! repopulate. Writes are queued on a worker task that applies them to the
//! master (or a caller-specified alternate target) and invalidates the
//! cache on success. An optional [`LiveStream`] also invalidates the cache
//! whenever an external event source observes a change.
//!
//! See `DESIGN.md` for the full architectural rationale.

mod event_consumer;
pub mod impls;
mod worker;
mod write_op;

use std::sync::Arc;

use tokio::sync::mpsc;
use vantage_table::any::AnyTable;
use vantage_table::pagination::Pagination;

use crate::cache::Cache;
use crate::live_stream::LiveStream;

pub use write_op::WriteOp;

/// Bounded queue capacity. Picked low because writes are infrequent
/// compared to reads; bumping it doesn't unlock new behaviour, just
/// hides backpressure.
const WRITE_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct LiveTable {
    /// The master table. `AnyTable` is itself `Clone`-cheap (clones the
    /// inner `Box<dyn TableLike>` via `clone_box`), so we hold it
    /// directly rather than behind a lock — `TableLike` requires sync
    /// metadata accessors that don't compose with `tokio::sync::RwLock`.
    /// Trade-off: the worker task captures its own clone at spawn time,
    /// so a future `set_master` would have to update both. v1 doesn't
    /// expose `set_master`.
    pub(crate) master: AnyTable,

    /// Caller-supplied identifier for the cached view. Combined with a
    /// page suffix to produce the actual cache key on each read.
    pub(crate) cache_key: String,

    /// Cache backend. `Arc<dyn Cache>` so the same backend instance can
    /// be shared by many `LiveTable`s pointing at different caches.
    pub(crate) cache: Arc<dyn Cache>,

    /// If set, write operations land here instead of the master. Reads
    /// stay on the master; only writes are diverted.
    pub(crate) custom_write_target: Option<AnyTable>,

    /// Channel into the write-queue worker task.
    pub(crate) write_queue: mpsc::Sender<WriteOp>,

    /// Optional event source — pushes fed in here invalidate the cache.
    /// Currently informational; the worker is wired up in a follow-up.
    #[allow(dead_code)]
    pub(crate) live_stream: Option<Arc<dyn LiveStream>>,

    /// The master's items-per-page ceiling. Stored for forward
    /// compatibility (multi-page glue when UI ipp > master ipp); v1
    /// trusts the caller to keep UI ipp at or below this.
    pub(crate) master_ipp: Option<i64>,

    /// Pagination state set by `TableLike::set_pagination`. Used to
    /// derive the cache key suffix on every read. Plain field, not
    /// behind a lock — `TableLike::get_pagination` is sync and returns
    /// a borrow, same shape `AnyTable` uses. Each `clone()` gets its
    /// own copy; master and cache stay shared.
    pub(crate) pagination: Option<Pagination>,
}

impl LiveTable {
    /// Build a `LiveTable` around `master`, caching results under
    /// `cache_key`. The worker task is spawned on the current tokio
    /// runtime; call this from within an async context.
    pub fn new(master: AnyTable, cache_key: impl Into<String>, cache: Arc<dyn Cache>) -> Self {
        let cache_key = cache_key.into();

        let (tx, rx) = mpsc::channel::<WriteOp>(WRITE_QUEUE_CAPACITY);

        // Spawn the worker. It owns the Receiver; when every Sender drops
        // (i.e. the LiveTable and all its clones), recv() returns None
        // and the worker loop exits cleanly.
        worker::spawn(
            rx,
            master.clone(),
            None, // custom_write_target — overridden via builder
            cache_key.clone(),
            Arc::clone(&cache),
        );

        Self {
            master,
            cache_key,
            cache,
            custom_write_target: None,
            write_queue: tx,
            live_stream: None,
            master_ipp: None,
            pagination: None,
        }
    }

    /// Set the master's max items-per-page hint. Stored, not enforced
    /// in v1 (caller is responsible for keeping UI ipp at or below).
    pub fn with_master_ipp(mut self, ipp: i64) -> Self {
        self.master_ipp = Some(ipp);
        self
    }

    /// Route writes to a different table than the master. Reads stay on
    /// the master; only writes are diverted. Setting this rebuilds the
    /// worker so it picks up the new target.
    pub fn with_custom_write_target(mut self, target: AnyTable) -> Self {
        // Drop the old worker by replacing the channel; old Sender drops
        // → old Receiver gets None → old worker exits.
        let (tx, rx) = mpsc::channel::<WriteOp>(WRITE_QUEUE_CAPACITY);
        worker::spawn(
            rx,
            self.master.clone(),
            Some(target.clone()),
            self.cache_key.clone(),
            Arc::clone(&self.cache),
        );
        self.write_queue = tx;
        self.custom_write_target = Some(target);
        self
    }

    /// Attach a live event source. Spawns a background task that
    /// subscribes to the stream and invalidates the cache on every
    /// event (sloppy invalidation — see DESIGN.md).
    pub fn with_live_stream(mut self, stream: Arc<dyn LiveStream>) -> Self {
        event_consumer::spawn(
            Arc::clone(&stream),
            self.cache_key.clone(),
            Arc::clone(&self.cache),
        );
        self.live_stream = Some(stream);
        self
    }

    /// The cache key used for a given page number. Public for
    /// observability / debugging — there's no production reason to call
    /// this from outside the crate.
    pub fn page_cache_key(&self, page: i64) -> String {
        format!("{}/page_{}", self.cache_key, page)
    }

    /// The cache key used for a single-row `get_value` lookup.
    pub fn id_cache_key(&self, id: &str) -> String {
        format!("{}/id/{}", self.cache_key, id)
    }
}

impl std::fmt::Debug for LiveTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveTable")
            .field("cache_key", &self.cache_key)
            .field("master_ipp", &self.master_ipp)
            .field(
                "has_custom_write_target",
                &self.custom_write_target.is_some(),
            )
            .field("has_live_stream", &self.live_stream.is_some())
            .finish()
    }
}
