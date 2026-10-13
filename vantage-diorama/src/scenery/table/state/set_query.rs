//! Changing the view's query: sort, filter chips, operator terms and search.
//!
//! Each setter writes its slot, recomputes the view's plan, and reloads the
//! view the way its shape needs: a paged view refetches the rows on screen
//! with the new query, an eager or two-pass one re-derives them locally.

use super::super::helpers::op_conditions_key;
use super::super::plan::Serving;
use super::super::{OpCondition, SortDir};
use super::TableSceneryState;

impl TableSceneryState {
    pub(crate) fn set_sort(&self, column: Option<String>, dir: SortDir) {
        tracing::debug!(
            target: "vantage_diorama::sort",
            column = ?column,
            dir = ?dir,
            two_pass = self.two_pass,
            "set_sort",
        );
        self.deregister();
        *self.sort.write().unwrap() = column.clone().map(|c| (c, dir));
        let reloaded = self.replan();
        // Where the ordering will actually happen is the whole question: a
        // source that takes this order does it over the entire set; one that
        // doesn't leaves us sorting the rows we happen to hold, which is a
        // different answer as well as a slower one.
        match &column {
            Some(col) => crate::debug::tapline!(
                self.debug_tap,
                "sort",
                "by {} {} — {}",
                col,
                match dir {
                    SortDir::Asc => "ascending",
                    SortDir::Desc => "descending",
                },
                match self.serving() {
                    Serving::Windows => "pushed to the source".to_string(),
                    Serving::Cache => "sorted over the whole set held locally".to_string(),
                    Serving::Failed(failure) => format!("the view can't answer it — {failure}"),
                },
            ),
            None => crate::debug::tapline!(self.debug_tap, "sort", "cleared"),
        }
        self.publish_view_stats();
        // The reactor's reseed decides how a new order reaches the rows: see
        // `reseed_from_cache`.
        if !reloaded {
            self.reload_notify.notify_one();
        }
    }

    pub(crate) fn set_filters(&self, filters: Vec<(String, ciborium::Value)>) {
        self.deregister();
        *self.ui_filters.write().unwrap() = filters;
        self.refilter();
    }

    pub(crate) fn set_filter_terms(&self, terms: Vec<OpCondition>) {
        // Order-insensitive: the same set restated is not a change.
        if op_conditions_key(&self.ui_terms.read().unwrap()) == op_conditions_key(&terms) {
            return;
        }
        crate::debug::tapline!(
            self.debug_tap,
            "filter",
            "{} term{} — {}",
            terms.len(),
            if terms.len() == 1 { "" } else { "s" },
            if self.paged {
                "pushed to the source where it can take them".to_string()
            } else {
                format!(
                    "filtering the {} rows held locally",
                    crate::debug::num(self.rows.read().unwrap().len())
                )
            },
        );
        self.deregister();
        *self.ui_terms.write().unwrap() = terms;
        self.refilter();
    }

    pub(crate) fn set_search(&self, query: Option<String>) {
        let normalized = query.filter(|q| !q.trim().is_empty());
        if *self.search.read().unwrap() == normalized {
            return;
        }
        tracing::debug!(
            target: "vantage_diorama::search",
            query = ?normalized,
            paged = self.paged,
            "set_search",
        );
        match &normalized {
            Some(q) => crate::debug::tapline!(self.debug_tap, "search", "\"{}\"", q),
            None => crate::debug::tapline!(self.debug_tap, "search", "cleared"),
        }
        // Reshaping a shared view is not this consumer's to do — same rule
        // as `set_sort`/`set_filters`.
        self.deregister();
        *self.search.write().unwrap() = normalized;
        let reloaded = self.replan();
        self.publish_view_stats();
        // A view serving windows keeps its rows and count on screen and
        // refetches the viewport in place (stale-while-revalidate): the
        // refetch carries the new search and overwrites each slot as rows
        // land, and the counted response then updates the total. Clearing
        // instead would blank the grid for the whole round trip on every
        // keystroke. Essential: the rows on screen answer the previous query,
        // and the user typed this one. A view served from the cache applies
        // the search there.
        if !reloaded {
            self.reload_view(vantage_core::Priority::Essential);
        }
    }

    /// Reload the view after its filter chips or operator terms changed.
    fn refilter(&self) {
        let reloaded = self.replan();
        // The cached total belongs to the unfiltered set. Clearing it also
        // republishes the snapshot with the new filters.
        self.set_total(None);
        // A view serving windows refetches the viewport with the new filters
        // (the same stale-while-revalidate as search; Essential, since the
        // rows on screen are the old filters' answer). A two-pass view's
        // membership lives in its index, which only `resort` re-derives —
        // refreshing the viewport would re-hydrate rows the new filters
        // exclude and leave them on screen — so the reactor routes it to
        // `resort`, and any other view to a cache reseed.
        if !reloaded {
            self.reload_view(vantage_core::Priority::Essential);
        }
    }
}
