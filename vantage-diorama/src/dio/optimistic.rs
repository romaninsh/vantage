//! Optimistic write path — stage a write in the cache and notify views
//! *before* the write-through confirms, then either commit or roll back.
//!
//! This is what makes form edits feel instant: the new value is visible the
//! moment the user hits save (rows flip to
//! [`PendingWrite`](crate::RowStatus::PendingWrite)), the actual write runs in
//! the background, and on failure the cache pre-image is restored and the row
//! flips to [`WriteFailed`](crate::RowStatus::WriteFailed). Because every view
//! reads the one cache row, the edit reflects across every bound scenery at
//! once.
//!
//! Contract: in this path the cache is **framework-managed**. The `on_write`
//! callback should be master-authoritative (write upstream); it may also touch
//! the cache, but the optimistic stage and the rollback are what views observe.

use vantage_core::Result;
use vantage_types::Record;

use ciborium::Value as CborValue;

use crate::dio::{Dio, DioEvent, DioInner};
use crate::ops::WriteOp;

impl Dio {
    /// Apply `op` optimistically: stage it in the cache, publish
    /// [`WritePending`](DioEvent::WritePending) so views show the new value as
    /// `PendingWrite`, run the write-through, then either confirm (publish
    /// [`RecordChanged`](DioEvent::RecordChanged), rows settle to `Fresh`) or
    /// restore the cache pre-image and publish
    /// [`WriteReverted`](DioEvent::WriteReverted) (rows revert and show
    /// `WriteFailed`).
    ///
    /// Returns `Ok(())` once committed, or the write-through's error after a
    /// successful rollback. `DeleteAll` has no single-row pre-image to stage,
    /// so it runs straight through the write-through with no optimism.
    pub async fn write_optimistic(&self, op: WriteOp) -> Result<()> {
        let Some(id) = op.id().map(str::to_string) else {
            return crate::dio::worker::run_write_through(self, op).await;
        };

        // 1. Snapshot the pre-image so a failed write can roll back exactly.
        let pre = self.inner.cache.get_value(&id).await?;

        // 2. Stage the optimistic value and announce it — views update now.
        apply_to_cache(&self.inner, &op, pre.as_ref()).await?;
        let _ = self
            .inner
            .event_bus
            .send(DioEvent::WritePending { id: id.clone() });

        // 3. Run the real write-through.
        match crate::dio::worker::run_write_through(self, op).await {
            Ok(()) => {
                let _ = self.inner.event_bus.send(DioEvent::RecordChanged { id });
                Ok(())
            }
            Err(err) => {
                // 4. Roll the cache back to the pre-image and surface the error.
                match &pre {
                    Some(prev) => self.inner.cache.insert_value(&id, prev).await?,
                    None => self.inner.cache.delete_value(&id).await?,
                }
                let _ = self.inner.event_bus.send(DioEvent::WriteReverted {
                    id,
                    error: err.to_string(),
                });
                Err(err)
            }
        }
    }

    /// Convenience for the common form-edit case: merge `partial` into the row
    /// at `id` optimistically.
    pub async fn patch_optimistic(
        &self,
        id: impl Into<String>,
        partial: Record<CborValue>,
    ) -> Result<()> {
        self.write_optimistic(WriteOp::Patch {
            id: id.into(),
            partial,
        })
        .await
    }
}

/// Write the op's optimistic result into the cache. `Patch` merges onto the
/// pre-image so untouched columns survive (the cache stores whole rows).
async fn apply_to_cache(
    inner: &DioInner,
    op: &WriteOp,
    pre: Option<&Record<CborValue>>,
) -> Result<()> {
    match op {
        WriteOp::Insert { id, record } | WriteOp::Replace { id, record } => {
            inner.cache.insert_value(id, record).await
        }
        WriteOp::Patch { id, partial } => {
            let mut merged = pre.cloned().unwrap_or_default();
            for (k, v) in partial {
                merged.insert(k.clone(), v.clone());
            }
            inner.cache.insert_value(id, &merged).await
        }
        WriteOp::Delete { id } => inner.cache.delete_value(id).await,
        WriteOp::DeleteAll => Ok(()),
    }
}
