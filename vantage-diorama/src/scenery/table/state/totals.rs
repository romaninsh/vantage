//! Grand total, row count and "more pages" answers.

use super::SharedRows;

impl SharedRows {
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
                self.write_id_to_idx(|id_to_idx| id_to_idx.retain(|_, idx| *idx < len));
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
        // A locally-refined view lists its whole index before it publishes
        // its visible set, so there is no further page to ask for.
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
}
