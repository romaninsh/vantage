mod builder;
mod cache;
pub(crate) mod cache_backend;
pub(crate) mod callbacks;
pub(crate) mod chunk_sink;
mod loader;
mod loader_cx;
pub(crate) mod memory_cache;
mod redb_cache;
mod refresher;
pub(crate) mod registry;
pub(crate) mod spawn;
mod tuning;

use std::sync::Arc;

use tokio::runtime::Handle;

pub use builder::LensBuilder;
pub use cache::Cache;
pub use cache_backend::{CacheBackend, CacheStatus, CacheTable};
pub use chunk_sink::ChunkSink;
pub(crate) use chunk_sink::{ChunkWrite, SceneryChunkTarget};
pub use loader::Loader;
pub use loader_cx::LoaderCx;
pub use memory_cache::{MemoryCache, MemoryCacheTable};
pub use redb_cache::{RedbCache, RedbCacheTable};
pub use refresher::Refresher;
pub use tuning::Tuning;

use callbacks::LensCallbacks;

/// Long-lived shared infrastructure for caching, loading, refreshing and
/// writing.
///
/// Built once through [`Lens::builder`] and shared by every Dio built from
/// it. A built Lens is immutable.
pub struct Lens {
    pub(crate) cache: Arc<dyn CacheBackend>,
    pub(crate) callbacks: LensCallbacks,
    tuning: Tuning,
    pub(crate) runtime: Handle,
    /// The debug stream's tap. Off unless the `diagnostics` feature's
    /// `LensBuilder::debug_datasource` enables it.
    pub(crate) debug: crate::debug::DebugTap,
    /// The live Dios built on this Lens, so a second build of one identity
    /// shares the first instead of starting another.
    pub(crate) dios: registry::DioRegistry,
}

impl Lens {
    pub fn builder() -> LensBuilder {
        LensBuilder::new()
    }

    /// Start building a [`Dio`](crate::Dio) over `vista` on this Lens. Its
    /// cache table is derived from the vista's name and applied query, so a
    /// vista narrowed before this call gets a table of its own.
    pub fn dio(self: &Arc<Self>, vista: vantage_vista::Vista) -> crate::dio::DioBuilder {
        crate::dio::DioBuilder::new(self.clone(), vista)
    }

    pub(crate) fn tuning(&self) -> &Tuning {
        &self.tuning
    }
}
