//! Rebuilding the visible row map from the cache.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::scenery::enriched_record::EnrichedRecord;

use super::super::helpers::{
    cmp_sort, matches_conditions, matches_op_conditions, matches_search, record_get_path,
};
use super::TableSceneryState;

impl TableSceneryState {
    /// Replace the sparse map from a freshly-listed cache snapshot.
    /// Applies conditions/sort in memory (v1-compat path).
    pub(crate) async fn reseed_from_cache(&self) -> vantage_core::Result<()> {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return Ok(());
        };
        let all = self.reader(&dio_inner).list_values().await?;

        let conditions = self.conditions.read().unwrap().clone();
        let op_conditions = self.op_conditions.read().unwrap().clone();
        // The UI's own filter set (grid chips, toolbar filter panel). Applied
        // here as well as on the two-pass path, because `local_refine` — which
        // gates that path — is false for a single-pass scenery, so a table
        // with no augmentation and no detail loader would take `set_filters`
        // and narrow nothing.
        let ui_filters = self.ui_filters.read().unwrap().clone();
        let ui_terms = self.ui_terms.read().unwrap().clone();
        let sort = self.sort.read().unwrap().clone();
        let search = self.search.read().unwrap().clone();

        let mut filtered: Vec<(String, Record<CborValue>)> = all
            .into_iter()
            .filter(|(_, rec)| matches_conditions(rec, &conditions))
            .filter(|(_, rec)| matches_conditions(rec, &ui_filters))
            .filter(|(_, rec)| matches_op_conditions(rec, &op_conditions))
            .filter(|(_, rec)| matches_op_conditions(rec, &ui_terms))
            .filter(|(_, rec)| matches_search(rec, search.as_deref()))
            .collect();

        // A paged view whose MASTER does the ordering cannot position cached
        // rows after the order changes. The cache holds an arbitrary subset of
        // the previous order — the windows this view happened to visit — and
        // re-sorting that subset locally would place it at rows 0..N of the new
        // order, which it is not: row 0 of "by name descending" is somewhere in
        // the 199,700 rows never fetched. The result is a grid that looks sorted
        // at the top and is wrong everywhere, mixed with correctly-positioned
        // rows wherever a later fetch landed.
        //
        // So drop the positions and keep the cache. The loader refills the
        // visible window from the master in the new order; the cached records
        // are still valid as values, just not as places.
        // Two-pass views are excluded: their positions come from the query
        // index, not from master windows, and `two_pass::resort` already
        // rebuilds that index. Clearing here would drop a spine that nothing
        // in this path refills.
        //
        // ...and only while the cache holds a SUBSET. A view that has fetched
        // the whole set can order it locally and be right, which is both
        // cheaper and instant; the hazard is strictly about ordering a sample
        // and presenting it as the whole.
        //
        // Count MATCHING rows, not cached rows: `total` describes the narrowed
        // set, so a cache full of rows the search excludes would otherwise
        // clear the bar while the matching rows are still a sample.
        let holds_everything = match *self.total.read().unwrap() {
            Some(total) => filtered.len() >= total,
            None => false,
        };
        if self.paged
            && !self.two_pass
            && self.master_capabilities.can_order
            && sort.is_some()
            && !holds_everything
        {
            self.rows.write().unwrap().clear();
            self.id_to_idx.write().unwrap().clear();
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

        if let Some((col, dir)) = sort {
            let missing = filtered
                .iter()
                .filter(|(_, r)| record_get_path(r, &col).is_none())
                .count();
            filtered.sort_by(|(_, a), (_, b)| {
                cmp_sort(record_get_path(a, &col), record_get_path(b, &col), dir)
            });
            tracing::debug!(
                target: "vantage_diorama::sort",
                col = %col,
                dir = ?dir,
                rows = filtered.len(),
                rows_missing_sort_value = missing,
                "reseed_from_cache applied sort",
            );
        }

        let mut rows = BTreeMap::new();
        let mut id_to_idx = HashMap::new();
        for (idx, (id, rec)) in filtered.into_iter().enumerate() {
            rows.insert(idx, Arc::new(EnrichedRecord::fresh(rec)));
            id_to_idx.insert(id, idx);
        }
        *self.rows.write().unwrap() = super::super::row_map::RowMap::from_map(rows);
        *self.id_to_idx.write().unwrap() = id_to_idx;
        Ok(())
    }
}
