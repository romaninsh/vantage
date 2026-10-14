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
            let Some(placed) = place(
                &rows,
                &id_to_idx,
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
        }
        self.bump_generation();
        Ok(true)
    }
}

/// The slot `rec` takes once row `id` has left its old one: `Some(None)`
/// when it doesn't stay, `None` when the rows aren't dense.
fn place(
    rows: &RowMap,
    id_to_idx: &HashMap<String, usize>,
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
    let mut ids: Vec<Option<&str>> = vec![None; n];
    for (other, &idx) in id_to_idx {
        *ids.get_mut(idx)? = Some(other.as_str());
    }
    let others = ids
        .into_iter()
        .zip(rows.values())
        .map(|(other, row)| Some((other?, &row.record)))
        .filter(|pair| pair.is_none_or(|(other, _)| other != id))
        .collect::<Option<Vec<_>>>()?;
    let at = others
        .partition_point(|&other| query.cmp_rows(other, (id, rec), metadata) == Ordering::Less);
    Some(Some(at))
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
