pub mod activity;
pub mod build;
pub mod cache_backend;
pub mod callbacks;
pub mod chunk_sink;
pub mod defaults;
pub mod make_dio;
pub mod memory_cache;
pub mod redb_cache;

use std::ops::Range;
use std::sync::Arc;

use tokio::runtime::Handle;

use std::future::Future;

use vantage_core::Result;

use crate::dio::Dio;
use crate::error::LensBuildError;
use crate::ops::{ChangeEvent, QueryDescriptor, WriteOp};

pub use activity::{Activity, ActivitySignal};
pub use cache_backend::{CacheBackend, CacheStatus, CacheTable};
pub use callbacks::{
    DioCallback, DioEventCallback, DioListPageCallback, DioLoadChunkCallback,
    DioLoadDetailCallback, DioTotalProviderCallback, DioWriteCallback, LensCallbacks,
    boxed_dio_callback, boxed_dio_event_callback, boxed_dio_write_callback,
    boxed_list_page_callback, boxed_load_chunk_callback, boxed_load_detail_callback,
    boxed_total_provider_callback,
};
pub use chunk_sink::{ChunkRow, ChunkSink, SceneryChunkTarget};
pub use defaults::LensDefaults;
pub use memory_cache::{MemoryCache, MemoryCacheTable};
pub use redb_cache::{RedbCache, RedbCacheTable};

/// Long-lived shared infrastructure for caching, callbacks, and refresh.
///
/// Built once via [`LensBuilder`] and shared across every Dio produced by
/// [`make_dio`](Lens::make_dio). After construction the Lens is immutable.
pub struct Lens {
    pub(crate) cache_source: Arc<dyn CacheBackend>,
    pub(crate) callbacks: Arc<LensCallbacks>,
    pub(crate) defaults: LensDefaults,
    pub(crate) runtime: Handle,
    /// App-activity signal driving adaptive refresh cadence. Shared with the UI
    /// (cloned), so flipping it re-paces every Dio's refresh loop at once.
    pub(crate) activity: ActivitySignal,
}

impl Lens {
    /// Start building a Lens. Equivalent to [`LensBuilder::new`].
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> LensBuilder {
        LensBuilder::new()
    }

    pub(crate) fn cache_source(&self) -> &Arc<dyn CacheBackend> {
        &self.cache_source
    }

    pub(crate) fn callbacks(&self) -> &Arc<LensCallbacks> {
        &self.callbacks
    }

    pub fn defaults(&self) -> &LensDefaults {
        &self.defaults
    }

    pub(crate) fn runtime(&self) -> &Handle {
        &self.runtime
    }
}

/// Configuration surface used to assemble a [`Lens`].
///
/// Setters are chainable; `.build()` validates required state and returns
/// a `Lens`. Stage 1 holds the shape only — the validation and dispatch
/// machinery lands in later stages.
pub struct LensBuilder {
    pub(crate) cache_source: Option<Arc<dyn CacheBackend>>,
    pub(crate) deferred_cache_error: Option<LensBuildError>,
    pub(crate) on_start: Option<DioCallback>,
    pub(crate) on_refresh: Option<DioCallback>,
    pub(crate) on_write: Option<DioWriteCallback>,
    pub(crate) on_event: Option<DioEventCallback>,
    pub(crate) total_provider: Option<DioTotalProviderCallback>,
    pub(crate) on_load_chunk: Option<DioLoadChunkCallback>,
    pub(crate) on_list_page: Option<DioListPageCallback>,
    pub(crate) on_load_detail: Option<DioLoadDetailCallback>,
    pub(crate) defaults: LensDefaults,
    pub(crate) runtime: Option<Handle>,
    pub(crate) activity: ActivitySignal,
}

impl Default for LensBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LensBuilder {
    pub fn new() -> Self {
        Self {
            cache_source: None,
            deferred_cache_error: None,
            on_start: None,
            on_refresh: None,
            on_write: None,
            on_event: None,
            total_provider: None,
            on_load_chunk: None,
            on_list_page: None,
            on_load_detail: None,
            defaults: LensDefaults::default(),
            runtime: None,
            activity: ActivitySignal::new(),
        }
    }

    /// Share an app-activity signal so this Lens's refresh loops adapt their
    /// cadence (active → fast, standby → slow, offline → paused). Pass the same
    /// cloned handle to every Lens and update it from the UI.
    pub fn activity_signal(mut self, signal: ActivitySignal) -> Self {
        self.activity = signal;
        self
    }

    /// The slower refresh interval used while the app is on
    /// [`Standby`](Activity::Standby). Falls back to the active
    /// [`refresh_every`](Self::refresh_every) interval when unset.
    pub fn standby_refresh_every(mut self, interval: std::time::Duration) -> Self {
        self.defaults.standby_refresh_interval = Some(interval);
        self
    }

    /// Provide the cache backend explicitly. Use this when [`cache_at`](Self::cache_at)
    /// is not flexible enough (e.g. wrapping a remote object store).
    pub fn cache_source(mut self, source: Arc<dyn CacheBackend>) -> Self {
        self.cache_source = Some(source);
        self
    }

    /// Convenience: cache to a redb file at `path`. Each Dio under the
    /// resulting Lens claims a named table within that file. Errors
    /// from opening redb propagate at [`build`](Self::build) time —
    /// the constructor is fallible but stored eagerly so `.build()`
    /// can decide what to do.
    pub fn cache_at(self, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        match RedbCache::open(&path) {
            Ok(cache) => self.cache_source(Arc::new(cache)),
            Err(e) => Self {
                deferred_cache_error: Some(LensBuildError::Other(e)),
                ..self
            },
        }
    }

    /// Convenience: cache to a process-local in-memory store. No file, no
    /// persistence — handy for tests and ephemeral Dios. Mirrors
    /// [`cache_at`](Self::cache_at)'s per-Dio-named-table + status semantics.
    pub fn cache_in_memory(self) -> Self {
        self.cache_source(Arc::new(MemoryCache::new()))
    }

    /// Register the `on_start` callback. Fires once when a Dio is built
    /// via [`Lens::make_dio`]; by default `make_dio` awaits it.
    ///
    /// The canonical shape is `|dio| { let dio = dio.clone(); async
    /// move { ... } }` — cloning Dio inside the closure produces a
    /// `'static` future without lifetime gymnastics.
    pub fn on_start<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_start = Some(boxed_dio_callback(f));
        self
    }

    /// Register the `on_refresh` callback. Fires on the configured
    /// [`refresh_every`](Self::refresh_every) interval and on manual
    /// `dio.refresh().await`.
    pub fn on_refresh<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_refresh = Some(boxed_dio_callback(f));
        self
    }

    /// Register the `on_write` callback. Fires for every WriteOp the
    /// Dio's write queue receives. When not registered, the worker
    /// applies the op directly to `dio.master()`.
    pub fn on_write<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio, WriteOp) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_write = Some(boxed_dio_write_callback(f));
        self
    }

    /// Register the `on_event` callback. Fires when an upstream
    /// [`ChangeEvent`] arrives (e.g. from a SurrealDB LIVE stream).
    pub fn on_event<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio, ChangeEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_event = Some(boxed_dio_event_callback(f));
        self
    }

    /// Register the `total_provider` callback. Fires once per
    /// [`TableScenery`](crate::scenery::TableScenery) open; the result
    /// drives `row_count()` and `estimated_total()` for that scenery's
    /// lifetime. Absent → `row_count` falls back to the cached map
    /// size (v1 behaviour).
    pub fn total_provider<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<usize>> + Send + 'static,
    {
        self.total_provider = Some(callbacks::boxed_total_provider_callback(f));
        self
    }

    /// Register the `on_load_chunk` callback. The Scenery calls this
    /// from `set_viewport` / `request_load_more` when the requested
    /// range is not fully cached. The callback fetches the rows from
    /// the master (or any other source) and streams them back via
    /// [`ChunkSink::push`]. Absent → viewport calls only emit
    /// `ViewportChanged` and never load.
    pub fn on_load_chunk<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio, Range<usize>, Option<(String, crate::SortDir)>, ChunkSink) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_load_chunk = Some(callbacks::boxed_load_chunk_callback(f));
        self
    }

    /// Register the two-pass **list pass**. The Scenery calls this to fetch one
    /// page of cheap/list rows for its query variant (conditions + sort +
    /// `offset`/`limit` arrive via the [`QueryDescriptor`]). Returned rows are
    /// written to the detail table as `Incomplete` and their ids appended to
    /// the per-query index. A page shorter than `limit` ends paging.
    ///
    /// Pairs with [`on_load_detail`](Self::on_load_detail); registering
    /// `on_load_detail` is what engages two-pass loading.
    pub fn on_list_page<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio, QueryDescriptor) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<(String, vantage_types::Record<ciborium::Value>)>>>
            + Send
            + 'static,
    {
        self.on_list_page = Some(callbacks::boxed_list_page_callback(f));
        self
    }

    /// Register the two-pass **detail pass**. The Scenery calls this once per
    /// visible `Incomplete` row to fetch its expensive columns; the returned
    /// record is merged into the detail table as `Complete` and the row flips
    /// to `Fresh`. **Registering this callback opts the Dio into two-pass
    /// loading** — without it, sceneries use the legacy single-pass path.
    pub fn on_load_detail<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio, String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<vantage_types::Record<ciborium::Value>>> + Send + 'static,
    {
        self.on_load_detail = Some(callbacks::boxed_load_detail_callback(f));
        self
    }

    pub fn refresh_every(mut self, interval: std::time::Duration) -> Self {
        self.defaults.refresh_interval = Some(interval);
        self
    }

    pub fn cache_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.defaults.cache_ttl = Some(ttl);
        self
    }

    pub fn write_queue_capacity(mut self, cap: usize) -> Self {
        self.defaults.write_queue_capacity = cap;
        self
    }

    pub fn on_start_blocking(mut self, blocking: bool) -> Self {
        self.defaults.on_start_blocking = blocking;
        self
    }

    /// Override the `refresh_on_open` default for sceneries opened
    /// from any Dio of this Lens.
    pub fn refresh_on_open(mut self, enabled: bool) -> Self {
        self.defaults.refresh_on_open = enabled;
        self
    }

    /// Override the viewport-debounce window.
    pub fn viewport_debounce(mut self, window: std::time::Duration) -> Self {
        self.defaults.viewport_debounce = window;
        self
    }

    /// Number of concurrent per-row augment detail fetches (the scheduler's
    /// worker pool). Default 1 — deterministic round-robin order across the
    /// views demanding rows; raise for parallel hydration.
    pub fn augment_workers(mut self, workers: usize) -> Self {
        self.defaults.augment_workers = workers;
        self
    }

    pub fn runtime(mut self, handle: Handle) -> Self {
        self.runtime = Some(handle);
        self
    }
}
