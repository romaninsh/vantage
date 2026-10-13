//! The view's query: local refinement, the two-pass index and the dedup registry.

use std::sync::Arc;

use super::TableSceneryState;

impl TableSceneryState {
    /// Whether the visible set is *locally refined* — filtered and ordered over
    /// the cache rather than served in the index's own order.
    ///
    /// Engaged when a two-pass view's query holds any clause — a condition, a
    /// filter chip, a search or an order: the list pass pushes none of them
    /// (and an augmented column doesn't exist until a row hydrates), so the
    /// visible map — not the index — is authoritative for `row_count`.
    ///
    /// **Derived, never stored**: a sort or filter chips set after open change
    /// the answer, and a flag computed at open would ignore them.
    ///
    /// The Dio's residual counts too: the master never applied it, so the
    /// index holds rows outside the Dio.
    ///
    /// A titles-only picker refines too: its list rows carry the cheap
    /// columns its clauses can name. A clause on an augmented column, which
    /// a picker never hydrates, fails it instead (see `Serving::decide`).
    pub(crate) fn local_refine(&self) -> bool {
        if !self.two_pass {
            return false;
        }
        !self.view_query().is_empty()
            || self
                .dio_weak
                .upgrade()
                .is_some_and(|dio| dio.residual_narrows())
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
