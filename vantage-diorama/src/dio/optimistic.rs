//! Optimistic flash path — stage a [`ChangeFlash`] in the cache and notify
//! views *before* the write-through confirms, then either commit or roll back.
//!
//! This is what makes form edits feel instant: the new value is visible the
//! moment the user hits save (rows flip to
//! [`PendingWrite`](crate::RowStatus::PendingWrite)), the actual write runs in
//! the background, and on failure the cache pre-image is restored and the row
//! flips to [`WriteFailed`](crate::RowStatus::WriteFailed). Because every view
//! reads the one cache row, the edit reflects across every bound scenery at
//! once.
//!
//! Contract: in this path the cache is **framework-managed**. The `on_flash`
//! route should be master-authoritative (write upstream); it may also touch
//! the cache, but the optimistic stage and the rollback are what views observe.

use vantage_core::Result;
use vantage_types::Record;

use ciborium::Value as CborValue;

use crate::dio::{Dio, DioEvent, DioInner};
use crate::ops::{ChangeFlash, FlashKind};

impl Dio {
    /// Emit `flash` optimistically: stage it in the cache, publish
    /// [`WritePending`](DioEvent::WritePending) so views show the new value as
    /// `PendingWrite`, run the write-through, then either confirm (publish
    /// [`RecordChanged`](DioEvent::RecordChanged), rows settle to `Fresh`) or
    /// restore the cache pre-image and publish
    /// [`WriteReverted`](DioEvent::WriteReverted) (rows revert and show
    /// `WriteFailed`).
    ///
    /// Returns `Ok(())` once committed, or the write-through's error after a
    /// successful rollback. `Clear` has no single-row pre-image to stage,
    /// so it runs straight through the write-through with no optimism.
    ///
    /// The flash reaches the `on_flash` route with its pre-image filled: if
    /// the emitter didn't supply `before`, the cache snapshot taken here is
    /// attached, so routes always see a self-contained flash.
    pub async fn flash(&self, mut flash: ChangeFlash) -> Result<()> {
        let Some(id) = flash.id().map(str::to_string) else {
            return crate::dio::worker::run_write_through(self, flash).await;
        };

        // 1. Snapshot the pre-image so a failed write can roll back exactly,
        //    and complete the flash with it for downstream routes.
        let pre = self.inner.cache.get_value(&id).await?;
        flash.ensure_before(pre.as_ref());

        // 2. Stage the optimistic value and announce it — views update now.
        stage_in_cache(&self.inner, &flash, pre.as_ref()).await?;
        let _ = self
            .inner
            .event_bus
            .send(DioEvent::WritePending { id: id.clone() });

        // 3. Run the real write-through.
        match crate::dio::worker::run_write_through(self, flash).await {
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

    /// Convenience for the common form-edit case: merge `partial` into the
    /// row at `id` optimistically.
    pub async fn flash_patch(
        &self,
        id: impl Into<String>,
        partial: Record<CborValue>,
    ) -> Result<()> {
        self.flash(ChangeFlash::new(FlashKind::Patch, Some(id.into()), partial))
            .await
    }

    /// Insert a new row optimistically.
    pub async fn flash_insert(
        &self,
        id: impl Into<String>,
        record: Record<CborValue>,
    ) -> Result<()> {
        self.flash(ChangeFlash::insert(id, record)).await
    }

    /// Replace the row at `id` optimistically (drops absent fields).
    pub async fn flash_replace(
        &self,
        id: impl Into<String>,
        record: Record<CborValue>,
    ) -> Result<()> {
        self.flash(ChangeFlash::replace(id, record)).await
    }

    /// Delete the row at `id` optimistically.
    pub async fn flash_delete(&self, id: impl Into<String>) -> Result<()> {
        self.flash(ChangeFlash::delete(id)).await
    }
}

/// Write the flash's optimistic result into the cache. `Patch` merges onto
/// the pre-image so untouched columns survive (the cache stores whole rows).
async fn stage_in_cache(
    inner: &DioInner,
    flash: &ChangeFlash,
    pre: Option<&Record<CborValue>>,
) -> Result<()> {
    let Some(id) = flash.id() else {
        return Ok(());
    };
    match flash.kind() {
        FlashKind::Insert | FlashKind::Replace => inner.cache.insert_value(id, flash.patch()).await,
        FlashKind::Patch => {
            let mut merged = pre.cloned().unwrap_or_default();
            for (k, v) in flash.patch() {
                merged.insert(k.clone(), v.clone());
            }
            inner.cache.insert_value(id, &merged).await
        }
        FlashKind::Delete => inner.cache.delete_value(id).await,
        FlashKind::Clear => Ok(()),
    }
}
