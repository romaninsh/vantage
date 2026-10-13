//! [`Cache`]: which store a Lens keeps its Dios' rows in.

use std::path::PathBuf;
use std::sync::Arc;

use vantage_core::{Context as _, Result, error};

use super::cache_backend::CacheBackend;
use super::memory_cache::MemoryCache;
use super::redb_cache::RedbCache;

/// The store behind a Lens. Each Dio claims one named table in it.
pub struct Cache(Kind);

enum Kind {
    Redb(PathBuf),
    Memory,
    Backend(Arc<dyn CacheBackend>),
}

impl Cache {
    /// A redb file at `path`, created if absent. The file opens when the Lens
    /// is built; a file that fails to open fails the build.
    pub fn redb(path: impl Into<PathBuf>) -> Self {
        Self(Kind::Redb(path.into()))
    }

    /// A process-local store with no file: nothing survives the process.
    pub fn memory() -> Self {
        Self(Kind::Memory)
    }

    /// Any other [`CacheBackend`].
    pub fn backend(backend: Arc<dyn CacheBackend>) -> Self {
        Self(Kind::Backend(backend))
    }

    pub(crate) fn open(self) -> Result<Arc<dyn CacheBackend>> {
        Ok(match self.0 {
            Kind::Redb(path) => Arc::new(RedbCache::open(&path).context(error!(
                "the Lens cache failed to open",
                setting = "cache",
                path = path.display()
            ))?),
            Kind::Memory => Arc::new(MemoryCache::new()),
            Kind::Backend(backend) => backend,
        })
    }
}
