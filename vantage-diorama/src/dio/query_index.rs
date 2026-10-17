//! Per-query ordered index of record ids.
//!
//! Two-pass loading separates *ordering* from *detail*. The detail table
//! (id→record, shared by Vista name) holds the records; a `QueryIndex` holds
//! the ordered list of ids for **one** query variant — a view's builder
//! conditions + order, identified by [`index_key`].
//!
//! The Dio caches one `QueryIndex` per key (see
//! [`DioInner::query_index`](crate::dio::DioInner::query_index)) and shares it
//! across every scenery that opens with the same conditions/order. Runtime
//! filters never key it: a view refines them over its index, so switching
//! them on and off reuses the already-built index — zero list calls — while
//! the shared detail table means already-hydrated records are never
//! re-fetched.

mod listing;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

use vantage_vista::{Vista, VistaQuery};

use listing::Source;

/// The identity of one query variant over `vista`: its name plus the
/// [`VistaQuery::key`] of `query` (its conditions and order). Only
/// identical queries share a key; condition order doesn't matter.
pub(crate) fn index_key(vista: &Vista, query: &VistaQuery) -> String {
    format!("{}|{}", vista.name(), query.key())
}

/// Ordered ids for one query variant, plus the bookkeeping the sequential
/// (no-total) list pass needs: whether paging is exhausted, and where the
/// listing reads from.
#[derive(Default)]
pub(crate) struct QueryIndex {
    ids: RwLock<Vec<String>>,
    /// Set once the list pass sees a short/empty page — no more pages exist.
    complete: AtomicBool,
    /// Set when a page completed the index honestly: a short page, or the
    /// stated total reached. Not by [`mark_complete`](Self::mark_complete),
    /// which only guesses the source repeats itself.
    listed_to_end: AtomicBool,
    /// See [`listing`].
    source: Mutex<Option<Source>>,
    /// Inserts confirmed while the index was still being listed.
    deferred: Mutex<Vec<String>>,
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
    ///
    /// A `total` the source states completes the index once it holds that
    /// many ids, so a set that ends on a full page needs no empty one.
    pub(crate) fn append_page(
        &self,
        ids: Vec<String>,
        requested_limit: usize,
        total: Option<usize>,
    ) -> (usize, Vec<String>) {
        let fetched = ids.len();
        let mut guard = self.ids.write().unwrap();
        // Seed the seen-set from the index, then let each accepted id join
        // it — this also drops ids repeated WITHIN the incoming page.
        let mut seen: std::collections::HashSet<String> = guard.iter().cloned().collect();
        let appended: Vec<String> = ids
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect();
        let base = guard.len();
        guard.extend(appended.iter().cloned());
        let len = guard.len();
        drop(guard);
        if fetched < requested_limit || total.is_some_and(|t| len >= t) {
            self.listed_to_end.store(true, Ordering::SeqCst);
            self.complete.store(true, Ordering::SeqCst);
        }
        (base, appended)
    }

    /// Whether a page completed the index (see `listed_to_end`), so the
    /// index holds every id its source has.
    pub(crate) fn listed_to_end(&self) -> bool {
        self.listed_to_end.load(Ordering::SeqCst)
    }

    /// Append `id` at the end, where the master puts a new row. `false` when
    /// the index already holds it. Completeness is unchanged.
    pub(crate) fn push(&self, id: String) -> bool {
        let mut ids = self.ids.write().unwrap();
        if ids.contains(&id) {
            return false;
        }
        ids.push(id);
        true
    }

    /// Take `id` out of the index, returning the position it held. A
    /// deferred insert of `id` goes too, so the listing's end can't bring a
    /// deleted row back.
    pub(crate) fn remove(&self, id: &str) -> Option<usize> {
        self.deferred.lock().unwrap().retain(|d| d != id);
        let mut ids = self.ids.write().unwrap();
        let at = ids.iter().position(|i| i == id)?;
        ids.remove(at);
        Some(at)
    }

    /// True once a short/empty page has been seen — the list pass is done.
    pub(crate) fn is_complete(&self) -> bool {
        self.complete.load(Ordering::SeqCst)
    }

    /// Mark the index complete without a short page, when the list pass
    /// finds the source repeats what it gave (it ignores the offset), so the
    /// index can't grow.
    pub(crate) fn mark_complete(&self) {
        self.complete.store(true, Ordering::SeqCst);
    }
}
