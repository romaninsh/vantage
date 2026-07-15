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

    /// Whether `id` is indexed for this query variant.
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.ids.read().unwrap().iter().any(|i| i == id)
    }

    /// Append one list page's worth of ids to the end of the index and record
    /// that a page was fetched. A page with fewer FETCHED rows than
    /// `requested_limit` (including empty) marks the index
    /// [`complete`](Self::is_complete) — there is no next page.
    ///
    /// Ids the index already holds are skipped: the index is shared across
    /// every scenery of a query variant, and two opening concurrently can
    /// both fetch the same page (each scenery's list single-flight doesn't
    /// span sceneries) — appending both copies would duplicate every row.
    /// Returns `(base, appended)`: the position the new ids landed at and the
    /// ids that were actually new, for seeding the caller's sparse map.
    pub(crate) fn append_page(
        &self,
        ids: Vec<String>,
        requested_limit: usize,
    ) -> (usize, Vec<String>) {
        let fetched = ids.len();
        let mut guard = self.ids.write().unwrap();
        let appended: Vec<String> = {
            let existing: std::collections::HashSet<&str> =
                guard.iter().map(|s| s.as_str()).collect();
            ids.into_iter()
                .filter(|id| !existing.contains(id.as_str()))
                .collect()
        };
        let base = guard.len();
        guard.extend(appended.iter().cloned());
        drop(guard);
        self.list_pages_fetched.fetch_add(1, Ordering::SeqCst);
        if fetched < requested_limit {
            self.complete.store(true, Ordering::SeqCst);
        }
        (base, appended)
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
        let (base, appended) = idx.append_page(vec!["a".into(), "b".into(), "c".into()], 3);
        assert_eq!(base, 0);
        assert_eq!(appended, vec!["a", "b", "c"]);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.id_at(0).as_deref(), Some("a"));
        assert_eq!(idx.id_at(2).as_deref(), Some("c"));
        assert_eq!(idx.ids(), vec!["a", "b", "c"]);
    }

    #[test]
    fn full_page_does_not_complete_short_page_does() {
        let idx = QueryIndex::new();
        // A full page (page_len == requested_limit) leaves room for more.
        idx.append_page(vec!["a".into(), "b".into()], 2);
        assert!(!idx.is_complete(), "a full page must not end paging");
        assert_eq!(idx.list_pages_fetched(), 1);

        // A short page ends paging.
        idx.append_page(vec!["c".into()], 2);
        assert!(idx.is_complete(), "a short page must end paging");
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.list_pages_fetched(), 2);
    }

    #[test]
    fn empty_page_completes() {
        let idx = QueryIndex::new();
        idx.append_page(Vec::new(), 50);
        assert!(idx.is_complete(), "an empty page must end paging");
        assert_eq!(idx.len(), 0);
        assert_eq!(idx.list_pages_fetched(), 1);
    }

    /// Two sceneries racing to page the shared index both fetch the same
    /// window; the second append keeps only the novel ids.
    #[test]
    fn duplicate_ids_are_skipped_on_append() {
        let idx = QueryIndex::new();
        idx.append_page(vec!["a".into(), "b".into()], 2);
        let (base, appended) = idx.append_page(vec!["a".into(), "b".into(), "c".into()], 3);
        assert_eq!(base, 2, "novel ids land at the tail");
        assert_eq!(appended, vec!["c"]);
        assert_eq!(idx.ids(), vec!["a", "b", "c"], "no duplicates");
        // Completeness still judges the FETCHED page: 3 of 3 → maybe more.
        assert!(!idx.is_complete());
    }
}
