//! The view's query: local refinement, the two-pass index and the dedup registry.

use std::sync::Arc;

use super::TableSceneryState;

impl TableSceneryState {
    /// Whether the visible set is *locally refined* — filtered and ordered over
    /// the cache rather than served in the index's own order.
    ///
    /// Engaged when a two-pass view carries any condition, filter chip or sort:
    /// the list pass can't push those down (and an augmented column doesn't
    /// exist until a row hydrates), so the visible map — not the index — is
    /// authoritative for `row_count`.
    ///
    /// **Derived, never stored.** It used to be a `bool` computed once in the
    /// builder, which made `set_sort` on a view opened without one a permanent
    /// no-op: the flag stayed `false`, so every path that would have applied the
    /// new order skipped it. Filter chips, which also arrive after open, had
    /// needed their own wrapper around the same stale flag; deriving the whole
    /// answer subsumes that.
    pub(crate) fn local_refine(&self) -> bool {
        if !self.two_pass || self.titles_only {
            return false;
        }
        !self.conditions.read().unwrap().is_empty()
            || !self.op_conditions.read().unwrap().is_empty()
            || !self.ui_filters.read().unwrap().is_empty()
            || !self.ui_terms.read().unwrap().is_empty()
            || self.sort.read().unwrap().is_some()
    }

    /// Current two-pass index (cloned `Arc`), or `None` in single-pass mode.
    pub(crate) fn index(&self) -> Option<Arc<crate::dio::query_index::QueryIndex>> {
        self.index.read().unwrap().clone()
    }

    /// Re-point the two-pass index at a different query variant's ordered index.
    pub(crate) fn set_index(&self, index: Option<Arc<crate::dio::query_index::QueryIndex>>) {
        *self.index.write().unwrap() = index;
    }

    /// Drop this scenery's dedup-registry entry the first time it mutates its
    /// own query (sort, filter chips) in place. Idempotent: the key is taken once.
    pub(crate) fn deregister(&self) {
        let Some(key) = self.registry_key.lock().unwrap().take() else {
            return;
        };
        if let Some(dio) = self.dio_weak.upgrade() {
            dio.table_sceneries.lock().unwrap().remove(&key);
        }
    }
}
