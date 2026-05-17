pub mod build;
pub mod cache_backend;
pub mod callbacks;
pub mod defaults;

use std::sync::Arc;

use tokio::runtime::Handle;

pub use cache_backend::CacheBackend;
pub use callbacks::{
    DioCallback, DioEventCallback, DioQueryCallback, DioWriteCallback, LensCallbacks,
};
pub use defaults::LensDefaults;

/// Long-lived shared infrastructure for caching, callbacks, and refresh.
///
/// Built once via [`LensBuilder`] and shared across every Dio produced by
/// [`make_dio`](Lens::make_dio). After construction the Lens is immutable.
pub struct Lens {
    pub(crate) cache_source: Arc<dyn CacheBackend>,
    pub(crate) callbacks: Arc<LensCallbacks>,
    pub(crate) defaults: LensDefaults,
    pub(crate) runtime: Handle,
}

impl Lens {
    /// Start building a Lens. Equivalent to [`LensBuilder::new`].
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
    pub(crate) on_start: Option<DioCallback>,
    pub(crate) on_refresh: Option<DioCallback>,
    pub(crate) on_write: Option<DioWriteCallback>,
    pub(crate) on_event: Option<DioEventCallback>,
    pub(crate) on_query: Option<DioQueryCallback>,
    pub(crate) defaults: LensDefaults,
    pub(crate) runtime: Option<Handle>,
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
            on_start: None,
            on_refresh: None,
            on_write: None,
            on_event: None,
            on_query: None,
            defaults: LensDefaults::default(),
            runtime: None,
        }
    }

    /// Provide the cache backend explicitly. Use this when [`cache_at`](Self::cache_at)
    /// is not flexible enough (e.g. wrapping a remote object store).
    pub fn cache_source(mut self, source: Arc<dyn CacheBackend>) -> Self {
        self.cache_source = Some(source);
        self
    }

    /// Convenience: cache to a redb file at `path`. Wired in stage 2 once
    /// the redb-backed `CacheBackend` impl lands.
    pub fn cache_at(self, _path: impl Into<std::path::PathBuf>) -> Self {
        // Stage 2: construct a redb-backed CacheBackend and call `cache_source`.
        self
    }

    pub fn on_start(mut self, cb: DioCallback) -> Self {
        self.on_start = Some(cb);
        self
    }

    pub fn on_refresh(mut self, cb: DioCallback) -> Self {
        self.on_refresh = Some(cb);
        self
    }

    pub fn on_write(mut self, cb: DioWriteCallback) -> Self {
        self.on_write = Some(cb);
        self
    }

    pub fn on_event(mut self, cb: DioEventCallback) -> Self {
        self.on_event = Some(cb);
        self
    }

    pub fn on_query(mut self, cb: DioQueryCallback) -> Self {
        self.on_query = Some(cb);
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

    pub fn runtime(mut self, handle: Handle) -> Self {
        self.runtime = Some(handle);
        self
    }
}
