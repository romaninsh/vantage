use std::sync::{Arc, Weak};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use crate::lens::cache_backend::CacheTable;

/// Trait the calling Scenery implements so a `ChunkSink` can stuff a
/// freshly-fetched row into the right sparse-map slot. Decouples
/// `ChunkSink` (which lives in `lens`) from the concrete scenery
/// state type.
pub trait SceneryChunkTarget: Send + Sync {
    fn write_chunk_row(&self, idx: usize, id: String, record: Record<CborValue>);
}

/// Handle passed to `on_load_chunk` callbacks. Each [`push`](Self::push)
/// writes one row to the Dio's cache and binds it to a row index in
/// the calling `TableScenery`'s sparse map. Cheap to clone; the scenery
/// is held by `Weak`, so dropping it mid-load makes subsequent pushes
/// fail cleanly.
#[derive(Clone)]
pub struct ChunkSink {
    pub(crate) target: Weak<dyn SceneryChunkTarget>,
    pub(crate) cache: Arc<dyn CacheTable>,
}

impl std::fmt::Debug for ChunkSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChunkSink")
            .field("scenery_alive", &(self.target.upgrade().is_some()))
            .finish()
    }
}

impl ChunkSink {
    /// Insert one row into the cache and bind it to `idx` in the
    /// scenery's sparse map. The row is visible to `row(idx)` as soon
    /// as `push` resolves, but the scenery's generation only bumps
    /// once `on_load_chunk` returns (via the `RangeLoaded` emission).
    pub async fn push(
        &self,
        idx: usize,
        id: impl Into<String>,
        record: Record<CborValue>,
    ) -> Result<()> {
        let id = id.into();
        let Some(target) = self.target.upgrade() else {
            return Err(vantage_core::error!("ChunkSink: scenery dropped"));
        };
        self.cache.insert_value(&id, &record).await?;
        target.write_chunk_row(idx, id, record);
        Ok(())
    }
}

/// One row's worth of payload — exposed publicly for callers that
/// want to model the same shape (queueing rows for testing, etc.).
#[derive(Debug, Clone)]
pub struct ChunkRow {
    pub idx: usize,
    pub id: String,
    pub record: Record<CborValue>,
}
