//! One listing of an index: the source its pages read, chosen at the first
//! page and kept to the end, and the inserts that wait for it to finish.
//!
//! A source picked per page could switch mid-listing, between the cache's id
//! order and the master's, when a whole copy is stamped or expires meanwhile,
//! and splice two orders into one index.

use super::QueryIndex;
use crate::dio::pending::SettleMark;

/// Where an index's pages come from.
pub(crate) enum Source {
    /// The cache's whole copy.
    Cache,
    /// The master, with the settle mark taken before its first page, held
    /// through that page. A whole copy stamped from it spares every row a
    /// flash settled since.
    Master(Option<SettleMark>),
}

impl QueryIndex {
    /// Whether the listing reads the cache: `None` before its first page.
    pub(crate) fn reads_cache(&self) -> Option<bool> {
        let source = self.source.lock().unwrap();
        source.as_ref().map(|s| matches!(s, Source::Cache))
    }

    /// Pin the listing's source unless an earlier first page pinned one:
    /// the cache when `cache`, else the master with `mark`. Returns whether
    /// the pinned source is the cache.
    pub(crate) fn pin_source(&self, cache: bool, mark: SettleMark) -> bool {
        let mut source = self.source.lock().unwrap();
        let pinned = source.get_or_insert_with(|| match cache {
            true => Source::Cache,
            false => Source::Master(Some(mark)),
        });
        matches!(pinned, Source::Cache)
    }

    /// The mark of a listing from the master, once, after its first page;
    /// `None` for one read from the cache or already taken. Taking it closes
    /// the mark, so settles stop being recorded for it.
    pub(crate) fn end_listing(&self) -> Option<SettleMark> {
        match self.source.lock().unwrap().as_mut() {
            Some(Source::Master(mark)) => mark.take(),
            _ => None,
        }
    }

    /// Hold `id`, an insert confirmed while the index is still listed, until
    /// the listing ends (see [`flush_deferred`](Self::flush_deferred)).
    pub(crate) fn defer_insert(&self, id: String) {
        self.deferred.lock().unwrap().push(id);
    }

    /// Hold `old`'s deferred inserts too: this index re-lists the set `old`
    /// was listing, and replaces it. `old` keeps its own, for the rows still
    /// reading it.
    pub(crate) fn adopt_deferred(&self, old: &QueryIndex) {
        let carried = old.deferred.lock().unwrap().clone();
        self.deferred.lock().unwrap().extend(carried);
    }

    /// Push each deferred insert the listing didn't bring, at the end.
    /// Returns the ids pushed, in order.
    pub(crate) fn flush_deferred(&self) -> Vec<String> {
        let deferred = std::mem::take(&mut *self.deferred.lock().unwrap());
        deferred.into_iter().filter(|id| self.push(id.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::QueryIndex;
    use crate::dio::pending::PendingFlashes;

    #[test]
    fn the_first_pinned_source_holds_to_the_end_of_the_listing() {
        let flashes = Arc::new(PendingFlashes::new());
        let idx = QueryIndex::new();
        assert_eq!(idx.reads_cache(), None);
        assert!(!idx.pin_source(false, flashes.mark()));
        assert!(!idx.pin_source(true, flashes.mark()), "the master stays pinned");
        assert!(idx.end_listing().is_some());
        assert!(idx.end_listing().is_none(), "the mark is handed out once");

        let cached = QueryIndex::new();
        assert!(cached.pin_source(true, flashes.mark()));
        assert!(cached.end_listing().is_none(), "a cache listing has no mark");
    }

    #[test]
    fn deferred_inserts_join_the_end_once_unless_listed() {
        let idx = QueryIndex::new();
        idx.append_page(vec!["a".into()], 2, None);
        idx.defer_insert("b".into());
        idx.defer_insert("a".into());
        assert_eq!(idx.flush_deferred(), vec!["b"]);
        assert_eq!(idx.ids(), vec!["a", "b"]);
        assert!(idx.flush_deferred().is_empty());
    }

    #[test]
    fn a_removed_id_leaves_the_deferred_inserts() {
        let idx = QueryIndex::new();
        idx.append_page(vec!["a".into()], 2, None);
        idx.defer_insert("b".into());
        idx.defer_insert("c".into());
        assert_eq!(idx.remove("b"), None, "a deferred id isn't indexed yet");
        assert_eq!(idx.flush_deferred(), vec!["c"]);
        assert_eq!(idx.ids(), vec!["a", "c"]);
    }

    #[test]
    fn a_fresh_index_adopts_the_old_ones_deferred_inserts() {
        let old = QueryIndex::new();
        old.defer_insert("b".into());
        let fresh = QueryIndex::new();
        fresh.adopt_deferred(&old);
        fresh.append_page(vec!["a".into()], 2, None);
        assert_eq!(fresh.flush_deferred(), vec!["b"]);
        assert_eq!(fresh.ids(), vec!["a", "b"]);
        assert_eq!(old.flush_deferred(), vec!["b"], "the old index keeps its own");
    }

    #[test]
    fn only_a_page_completes_the_listing_to_its_end() {
        let guessed = QueryIndex::new();
        guessed.append_page(vec!["a".into()], 1, None);
        guessed.mark_complete();
        assert!(guessed.is_complete());
        assert!(!guessed.listed_to_end());

        let short = QueryIndex::new();
        short.append_page(vec!["a".into()], 2, None);
        assert!(short.listed_to_end());
    }
}
