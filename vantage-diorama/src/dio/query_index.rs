//! Per-query ordered index of record ids.
//!
//! Two-pass loading separates *ordering* from *detail*. The detail table
//! (id→record, shared by Vista name) holds the records; a `QueryIndex` holds
//! the ordered list of ids for **one** query variant — a specific combination
//! of conditions + sort, identified by
//! [`Vista::index_key`](vantage_vista::Vista::index_key).
//!
//! The Dio caches one `QueryIndex` per key (see
//! [`DioInner::query_index`](crate::dio::DioInner::query_index)) and shares it
//! across every scenery that opens with the same conditions/sort. Switching a
//! filter on and off therefore reuses the already-built index — zero list
//! calls — while the shared detail table means already-hydrated records are
//! never re-fetched.

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Ordered ids for one query variant, plus the bookkeeping the sequential
/// (no-total) list pass needs: whether paging is exhausted and how many list
/// pages have been fetched.
#[derive(Debug, Default)]
pub(crate) struct QueryIndex {
    ids: RwLock<Vec<String>>,
    /// Set once the list pass sees a short/empty page — no more pages exist.
    complete: AtomicBool,
    /// Number of list-page fetches that have populated this index. Drives
    /// diagnostics and the BDD invocation-count assertions.
    list_pages_fetched: AtomicUsize,
}

impl QueryIndex {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of ids currently indexed.
    pub(crate) fn len(&self) -> usize {
        self.ids.read().unwrap().len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Id at ordered position `idx`, if present.
    pub(crate) fn id_at(&self, idx: usize) -> Option<String> {
        self.ids.read().unwrap().get(idx).cloned()
    }

    /// Snapshot of all ids in order.
    pub(crate) fn ids(&self) -> Vec<String> {
        self.ids.read().unwrap().clone()
    }

    /// Append one list page's worth of ids to the end of the index and record
    /// that a page was fetched. `page_len` is the number of rows the source
    /// returned; a page shorter than `requested_limit` (including empty) marks
    /// the index [`complete`](Self::is_complete) — there is no next page.
    pub(crate) fn append_page(
        &self,
        ids: impl IntoIterator<Item = String>,
        requested_limit: usize,
    ) {
        let mut guard = self.ids.write().unwrap();
        let before = guard.len();
        guard.extend(ids);
        let page_len = guard.len() - before;
        drop(guard);
        self.list_pages_fetched.fetch_add(1, Ordering::SeqCst);
        if page_len < requested_limit {
            self.complete.store(true, Ordering::SeqCst);
        }
    }

    /// True once a short/empty page has been seen — the list pass is done.
    pub(crate) fn is_complete(&self) -> bool {
        self.complete.load(Ordering::SeqCst)
    }

    pub(crate) fn list_pages_fetched(&self) -> usize {
        self.list_pages_fetched.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_page_grows_index_and_preserves_order() {
        let idx = QueryIndex::new();
        assert!(idx.is_empty());
        idx.append_page(["a".into(), "b".into(), "c".into()], 3);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.id_at(0).as_deref(), Some("a"));
        assert_eq!(idx.id_at(2).as_deref(), Some("c"));
        assert_eq!(idx.ids(), vec!["a", "b", "c"]);
    }

    #[test]
    fn full_page_does_not_complete_short_page_does() {
        let idx = QueryIndex::new();
        // A full page (page_len == requested_limit) leaves room for more.
        idx.append_page(["a".into(), "b".into()], 2);
        assert!(!idx.is_complete(), "a full page must not end paging");
        assert_eq!(idx.list_pages_fetched(), 1);

        // A short page ends paging.
        idx.append_page(["c".into()], 2);
        assert!(idx.is_complete(), "a short page must end paging");
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.list_pages_fetched(), 2);
    }

    #[test]
    fn empty_page_completes() {
        let idx = QueryIndex::new();
        idx.append_page(Vec::<String>::new(), 50);
        assert!(idx.is_complete(), "an empty page must end paging");
        assert_eq!(idx.len(), 0);
        assert_eq!(idx.list_pages_fetched(), 1);
    }
}
