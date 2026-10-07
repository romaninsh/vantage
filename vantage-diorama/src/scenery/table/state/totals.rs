//! Grand total, row count and "more pages" answers.

use std::sync::atomic::Ordering;

use super::TableSceneryState;

impl TableSceneryState {
    /// Overwrite the cached grand total with an exact one. Returns `true` if
    /// it changed (so the loader can bump the generation for `row_count`
    /// consumers).
    pub(crate) fn set_total(&self, total: Option<usize>) -> bool {
        self.set_total_as(total, super::super::view_stats::TotalKind::Exact)
    }

    /// [`set_total`](Self::set_total) for a total that is not a count. The
    /// return value tracks the number only; a change of kind alone still
    /// republishes the snapshot.
    pub(crate) fn set_total_as(
        &self,
        total: Option<usize>,
        kind: super::super::view_stats::TotalKind,
    ) -> bool {
        let changed = {
            let mut guard = self.total.write().unwrap();
            let changed = *guard != total;
            *guard = total;
            changed
        };
        // A paged view's slots are positions in the source's order, so a
        // counted total ends them. Rows past it came from a warm cache that
        // outlived them, or from before the source shrank.
        if let (Some(len), super::super::view_stats::TotalKind::Exact) = (total, kind)
            && self.is_chunk_loaded()
        {
            let dropped = self.rows.write().unwrap().truncate(len);
            if !dropped.is_empty() {
                self.id_to_idx.write().unwrap().retain(|_, idx| *idx < len);
            }
        }
        *self.total_kind.write().unwrap() = kind;
        self.publish_view_stats();
        changed
    }

    /// See [`TableScenery::row_count`](super::super::TableScenery::row_count).
    pub(crate) fn row_count(&self) -> usize {
        // A locally-refined view's visible map is authoritative — the index may
        // hold more ids than match the filter.
        if self.local_refine() {
            return self.rows.read().unwrap().len();
        }
        if let Some(index) = self.index() {
            return index.len();
        }
        if let Some(t) = *self.total.read().unwrap() {
            return t;
        }
        self.rows.read().unwrap().len()
    }

    /// See [`TableScenery::has_more`](super::super::TableScenery::has_more).
    pub(crate) fn has_more(&self) -> bool {
        // A locally-refined view materializes its whole visible set from the
        // (already-listed) index, so there is no further page to ask for.
        if self.local_refine() {
            return false;
        }
        // Two-pass / sequential no-total: more pages exist until the list pass
        // sees a short or empty page.
        if let Some(index) = self.index() {
            return !index.is_complete();
        }
        let total = *self.total.read().unwrap();
        let loaded = self.rows.read().unwrap().len();
        match total {
            Some(t) => loaded < t,
            None => false,
        }
    }

    /// See [`TableScenery::status_summary`](super::super::TableScenery::status_summary).
    pub(crate) fn status_summary(&self) -> super::super::RowStatusSummary {
        self.rows.read().unwrap().summary()
    }

    /// Re-invoke the lens `total_provider` and update the cached grand total, so
    /// a row that appeared (or vanished) server-side since open grows (or shrinks)
    /// the scrollbar instead of staying frozen at the open-time total. No-op when
    /// no provider is registered (the total then self-corrects from short pages).
    ///
    /// Deliberately does NOT bump the generation: it runs at the *start* of a
    /// refresh, before the in-place refetch repopulates the rows. Bumping here
    /// would repaint an intermediate frame (new count, rows not yet refreshed /
    /// re-sorted) — a visible flicker. The forced refetch that follows carries
    /// the single repaint, so the new count and the refreshed+re-sorted rows land
    /// together.
    pub(crate) async fn refresh_total(&self) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        let Some(cb) = dio_inner.lens.callbacks.total_provider.as_ref() else {
            return;
        };
        let dio = crate::Dio {
            inner: dio_inner.clone(),
        };
        match cb(&dio).await {
            Ok(total) => {
                // Stated, not inferred — latch it so the unknown-total
                // horizon rule stays out of the way (see `total_ever_stated`).
                self.total_ever_stated.store(true, Ordering::SeqCst);
                self.set_total(Some(total));
                // Remember it for the next open's head start (skipped under
                // an active search — that total describes the narrowed set).
                if self.search.read().unwrap().is_none()
                    && let Err(e) = dio_inner.cache.set_meta_total(total as u64).await
                {
                    tracing::debug!(
                        target: "vantage_diorama::cache",
                        error = %e,
                        "persisting the provider total failed",
                    );
                }
            }
            Err(e) => tracing::error!(error = %e, "refresh_total failed"),
        }
    }
}
