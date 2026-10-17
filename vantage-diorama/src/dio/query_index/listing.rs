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
        deferred
            .into_iter()
            .filter(|id| self.push(id.clone()))
            .collect()
    }
}
