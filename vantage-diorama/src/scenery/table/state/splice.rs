//! Moving one changed row to its place in an ordered set held whole, in
//! place of a reseed that re-lists and re-sorts the cache.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering as AtomicOrdering;

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{VistaMetadata, VistaQuery};

use crate::scenery::enriched_record::EnrichedRecord;

use super::super::plan::{Serving, admitted_by_view};
use super::super::row_map::RowMap;
use super::SharedRows;

impl SharedRows {
    /// Whether a change to one row can be spliced in rather than reseeded:
    /// single-pass rows served whole from the cache, settled, in a declared
    /// order, and placed by a reseed for that order (copied rows aren't yet).
    /// Without an order the rows keep the cache's own order, which only a
    /// reseed reads.
    pub(crate) fn splices_in_place(&self) -> bool {
        !self.two_pass
            && self.query.order.is_some()
            && self.settled.load(AtomicOrdering::SeqCst)
            && self.placed.load(AtomicOrdering::SeqCst)
            && matches!(*self.serving.read().unwrap(), Serving::Cache)
    }

    /// Re-read row `id` from the cache and move it where
    /// [`VistaQuery::sort_rows`] would put it: out of its old slot, and into
    /// the slot a binary search by [`VistaQuery::cmp_rows`] finds when the
    /// view still admits it. Bumps once.
    ///
    /// Returns `false`, changing nothing, when the rows aren't the dense
    /// `0..n` a whole sorted set is; the caller reseeds instead.
    pub(crate) async fn splice_one(&self, id: &str) -> vantage_core::Result<bool> {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return Ok(true);
        };
        let began = self.serving();
        let fetched = self.reader(&dio_inner).get_value(id).await?;
        let query = self.view_query();
        let admitted = fetched.filter(|rec| admitted_by_view(&dio_inner, &query, rec));
        if !self.still_serving(&began) {
            return Ok(true);
        }
        {
            let mut rows = self.rows.write().unwrap();
            let mut id_to_idx = self.id_to_idx.write().unwrap();
            let mut idx_to_id = self.idx_to_id.lock().unwrap();
            let n = rows.len();
            let Some(placed) = place(
                &rows,
                &id_to_idx,
                &mut idx_to_id,
                &query,
                &dio_inner.metadata,
                id,
                admitted.as_ref(),
            ) else {
                return Ok(false);
            };
            let old = id_to_idx.remove(id);
            if let Some(old) = old {
                rows.remove_shift(old);
            }
            if let (Some(at), Some(rec)) = (placed, admitted) {
                rows.insert_shift(at, Arc::new(EnrichedRecord::fresh(rec)));
            }
            reindex(&mut id_to_idx, old, placed);
            if let Some(at) = placed {
                id_to_idx.insert(id.to_string(), at);
            }
            move_slot(&mut idx_to_id, n, id, old, placed);
        }
        self.bump_generation();
        Ok(true)
    }

    /// Change `id_to_idx` and forget [`idx_to_id`](Self::idx_to_id) under the
    /// same write lock, so no splice reads the old slots against the new map.
    pub(crate) fn write_id_to_idx<R>(
        &self,
        change: impl FnOnce(&mut HashMap<String, usize>) -> R,
    ) -> R {
        let mut id_to_idx = self.id_to_idx.write().unwrap();
        let out = change(&mut id_to_idx);
        self.forget_idx_to_id();
        out
    }

    /// Drop the slot index; the next splice rebuilds it from `id_to_idx`.
    pub(crate) fn forget_idx_to_id(&self) {
        *self.idx_to_id.lock().unwrap() = None;
    }
}

/// The slot `rec` takes once row `id` has left its old one: `Some(None)`
/// when it doesn't stay, `None` when the rows aren't dense. When `rec`
/// stays, `idx_to_id` is left current for the rows as they are now.
fn place(
    rows: &RowMap,
    id_to_idx: &HashMap<String, usize>,
    idx_to_id: &mut Option<Vec<String>>,
    query: &VistaQuery,
    metadata: &VistaMetadata,
    id: &str,
    rec: Option<&Record<CborValue>>,
) -> Option<Option<usize>> {
    let n = rows.len();
    if id_to_idx.len() != n || rows.keys().next_back().is_some_and(|last| *last + 1 != n) {
        return None;
    }
    let Some(rec) = rec else {
        return Some(None);
    };
    let old = id_to_idx.get(id).copied();
    if !idx_to_id.as_deref().is_some_and(|ids| current(ids, n, id, old)) {
        *idx_to_id = Some(slot_ids(id_to_idx, n)?);
    }
    let ids = idx_to_id.as_deref()?;
    // `partition_point` over the rows other than `old`, read in place.
    let slot = |i: usize| if old.is_some_and(|old| i >= old) { i + 1 } else { i };
    let (mut lo, mut hi) = (0, n - usize::from(old.is_some()));
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let at = slot(mid);
        let other = (ids[at].as_str(), &rows.get(&at)?.record);
        if query.cmp_rows(other, (id, rec), metadata) == Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Some(Some(lo))
}

/// Whether `ids` still names the slots of `n` rows, as far as row `id` at
/// `old` can tell.
fn current(ids: &[String], n: usize, id: &str, old: Option<usize>) -> bool {
    ids.len() == n && old.is_none_or(|old| ids.get(old).is_some_and(|at| at == id))
}

/// The id at each of `n` slots, or `None` when `id_to_idx` leaves a slot
/// empty or points past the end.
fn slot_ids(id_to_idx: &HashMap<String, usize>, n: usize) -> Option<Vec<String>> {
    let mut ids: Vec<Option<&str>> = vec![None; n];
    for (id, &idx) in id_to_idx {
        *ids.get_mut(idx)? = Some(id.as_str());
    }
    ids.into_iter().map(|id| id.map(str::to_string)).collect()
}

/// Shift the slots past `removed` down and those at or past `inserted` up,
/// as `remove_shift` then `insert_shift` moved the rows. Only the slots
/// between the two change.
fn reindex(
    id_to_idx: &mut HashMap<String, usize>,
    removed: Option<usize>,
    inserted: Option<usize>,
) {
    for idx in id_to_idx.values_mut() {
        if removed.is_some_and(|old| *idx > old) {
            *idx -= 1;
        }
        if inserted.is_some_and(|at| *idx >= at) {
            *idx += 1;
        }
    }
}

/// Move `id` in the slot index from `removed` to `inserted`, as the rows
/// moved. An index that wasn't current for the `n` rows before the move is
/// dropped instead.
fn move_slot(
    idx_to_id: &mut Option<Vec<String>>,
    n: usize,
    id: &str,
    removed: Option<usize>,
    inserted: Option<usize>,
) {
    let Some(ids) = idx_to_id.as_mut() else {
        return;
    };
    if !current(ids, n, id, removed) {
        *idx_to_id = None;
        return;
    }
    if let Some(old) = removed {
        ids.remove(old);
    }
    if let Some(at) = inserted {
        ids.insert(at, id.to_string());
    }
}

#[cfg(test)]
mod tests;
