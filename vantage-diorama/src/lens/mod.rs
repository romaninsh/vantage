pub mod build;
pub mod cache_backend;
pub mod callbacks;
pub mod defaults;
pub mod make_dio;
pub mod redb_cache;

use std::sync::Arc;

use tokio::runtime::Handle;

use std::future::Future;

use vantage_core::Result;

use crate::dio::Dio;
use crate::error::LensBuildError;
use crate::ops::{ChangeEvent, QueryDescriptor, WriteOp};

pub use cache_backend::{CacheBackend, CacheTable};
pub use callbacks::{
    DioCallback, DioEventCallback, DioQueryCallback, DioWriteCallback, LensCallbacks,
    boxed_dio_callback, boxed_dio_event_callback, boxed_dio_query_callback,
    boxed_dio_write_callback,
};
pub use defaults::LensDefaults;
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
    pub(crate) deferred_cache_error: Option<LensBuildError>,
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
            deferred_cache_error: None,
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

    /// Register the `on_query` callback. Stage 5b will wire this up;
    /// stage 3 only stores the registration.
    pub fn on_query<F, Fut>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio, QueryDescriptor) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.on_query = Some(boxed_dio_query_callback(f));
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
