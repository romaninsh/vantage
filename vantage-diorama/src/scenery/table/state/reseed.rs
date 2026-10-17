//! Rebuilding the visible row map from the cache.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::scenery::enriched_record::EnrichedRecord;

use super::super::plan::{Serving, admitted_by_view};
use super::SharedRows;
use crate::scenery::table::LoadFailure;

impl SharedRows {
    /// Replace the sparse map from a freshly-listed cache snapshot, with the
    /// whole view query and the Dio's residual evaluated over it.
    ///
    /// A paged view served from the cache first waits for the Dio's whole
    /// copy, so the cache it refines is the complete set. A failed view
    /// keeps showing nothing, and so does one whose serving a setter changed
    /// while this awaited: the reload that change triggered owns the rows.
    pub(crate) async fn reseed_from_cache(&self) -> vantage_core::Result<()> {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return Ok(());
        };
        let began = self.serving();
        match &began {
            Serving::Failed(_) => return Ok(()),
            Serving::Cache if self.paged && !self.two_pass => {
                if let Err(e) = dio_inner.copy_whole().await {
                    if self.still_serving(&began) {
                        let failure = LoadFailure::new(dio_inner.master.name(), e.to_string());
                        self.fail(&dio_inner, failure);
                    }
                    return Ok(());
                }
                if !self.still_serving(&began) {
                    return Ok(());
                }
            }
            _ => {}
        }
        let all = self.reader(&dio_inner).list_values().await?;

        let query = self.view_query();
        let mut filtered: Vec<(String, Record<CborValue>)> = all
            .into_iter()
            .filter(|(_, rec)| admitted_by_view(&dio_inner, &query, rec))
            .collect();

        // A paged view whose master does the ordering can't place cached rows
        // after the order changes: the cache holds the windows it happened to
        // visit, and re-sorting that sample would seat it at rows 0..N of the
        // new order, which it is not. So the positions go and the cache stays;
        // the loader refills the window in the new order. Two-pass views are
        // excluded: their positions come from the query index, which
        // `two_pass::resort` rebuilds.
        //
        // Only while the cache holds a sample: a view holding the whole set
        // orders it locally and is right. Matching rows are counted, not
        // cached ones, because `total` describes the narrowed set. A view
        // serving windows has no residual, so its order is the master's.
        let holds_everything = match *self.total.read().unwrap() {
            Some(total) => filtered.len() >= total,
            None => false,
        };
        // Filtering and sorting a large cache takes time a setter can use to
        // move the view on, so the serving is re-checked just before each
        // write.
        if !self.still_serving(&began) {
            return Ok(());
        }
        if self.is_chunk_loaded() && query.order.is_some() && !holds_everything {
            self.placed.store(false, Ordering::SeqCst);
            self.rows.write().unwrap().clear();
            self.write_id_to_idx(|ids| ids.clear());
            tracing::debug!(
                target: "vantage_diorama::sort",
                "paged view re-orders at the source — dropped cached row positions",
            );
            crate::debug::tapline!(
                self.debug_tap,
                "scenery",
                "row positions dropped — the source re-orders, so cached rows must be re-placed",
            );
            // Refill immediately rather than waiting for the consumer to
            // re-declare its viewport. A grid repaints and re-declares, so it
            // would recover either way; a consumer that holds its viewport
            // still would sit on an empty view forever.
            //
            // Essential: the positions were just dropped, so this refill is
            // the only thing that can put rows back in front of the user who
            // changed the order.
            self.refresh_loaded_viewport(vantage_core::Priority::Essential);
            return Ok(());
        }

        if let Some((col, dir)) = &query.order {
            query.sort_rows(&mut filtered, &dio_inner.metadata);
            tracing::debug!(
                target: "vantage_diorama::sort",
                col = %col,
                dir = ?dir,
                rows = filtered.len(),
                "reseed_from_cache applied sort",
            );
        }

        let mut rows = BTreeMap::new();
        let mut id_to_idx = HashMap::new();
        for (idx, (id, rec)) in filtered.into_iter().enumerate() {
            rows.insert(idx, Arc::new(EnrichedRecord::fresh(rec)));
            id_to_idx.insert(id, idx);
        }
        if !self.still_serving(&began) {
            return Ok(());
        }
        *self.rows.write().unwrap() = super::super::row_map::RowMap::from_map(rows);
        self.write_id_to_idx(|ids| *ids = id_to_idx);
        self.placed.store(true, Ordering::SeqCst);
        Ok(())
    }
}
