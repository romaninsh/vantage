//! A view's setters: order, runtime filters and search.
//!
//! Each writes the view's own slot under `repointing` and re-points the view
//! at the rows of its new query (see `repoint`). A refused tail records the
//! slot and stays put until an order lifts the refusal.

use vantage_vista::{Condition, SortDirection, VistaQuery};

use crate::scenery::table::plan::Serving;

use super::TableView;
use super::repoint::Change;
use super::tail::Refusal;

impl TableView {
    pub(crate) fn set_order(&self, order: Option<(String, SortDirection)>) {
        tracing::debug!(
            target: "vantage_diorama::sort",
            order = ?order,
            "set_order",
        );
        let _serial = self.repointing.lock().unwrap_or_else(|e| e.into_inner());
        // A tail keeps the Dio's order, as it opened with, when its own is
        // cleared.
        let order = match self.cap() {
            Some(cap) if cap.is_tail() => order.or_else(|| {
                let dio = self.dio_weak.upgrade();
                dio.and_then(|dio| dio.query.order.clone())
            }),
            _ => order,
        };
        *self.order.write().unwrap() = order.clone();
        match self.refuse_unordered_tail(order.is_none()) {
            Refusal::Refused => return,
            // The other clauses may have moved while it was refused.
            Refusal::Lifted => self.repoint(Change::Filters),
            Refusal::Unchanged => self.repoint(Change::Clause),
        }
        // Where the ordering will actually happen is the whole question: a
        // source that takes this order does it over the entire set; one that
        // doesn't leaves us sorting the rows we happen to hold, which is a
        // different answer as well as a slower one.
        let shared = self.shared();
        match &order {
            Some((col, dir)) => crate::debug::tapline!(
                shared.debug_tap,
                "sort",
                "by {} {} — {}",
                col,
                match dir {
                    SortDirection::Ascending => "ascending",
                    SortDirection::Descending => "descending",
                },
                match shared.serving() {
                    Serving::Windows => "pushed to the source".to_string(),
                    Serving::Cache => "sorted over the whole set held locally".to_string(),
                    Serving::Failed(failure) => format!("the view can't answer it — {failure}"),
                },
            ),
            None => crate::debug::tapline!(shared.debug_tap, "sort", "cleared"),
        }
    }

    pub(crate) fn set_filters(&self, filters: Vec<Condition>) {
        let _serial = self.repointing.lock().unwrap_or_else(|e| e.into_inner());
        // Order-insensitive: the same set restated is not a change.
        if conditions_key(&self.filters.read().unwrap()) == conditions_key(&filters) {
            return;
        }
        let shared = self.shared();
        crate::debug::tapline!(
            shared.debug_tap,
            "filter",
            "{} filter{} — {}",
            filters.len(),
            if filters.len() == 1 { "" } else { "s" },
            if shared.paged {
                "pushed to the source where it can take them".to_string()
            } else {
                format!(
                    "filtering the {} rows held locally",
                    crate::debug::num(shared.rows.read().unwrap().len())
                )
            },
        );
        *self.filters.write().unwrap() = filters;
        self.repoint_unless_refused(Change::Filters);
    }

    pub(crate) fn set_search(&self, query: Option<String>) {
        let _serial = self.repointing.lock().unwrap_or_else(|e| e.into_inner());
        let normalized = query.filter(|q| !q.trim().is_empty());
        if *self.search.read().unwrap() == normalized {
            return;
        }
        let shared = self.shared();
        tracing::debug!(
            target: "vantage_diorama::search",
            query = ?normalized,
            paged = shared.paged,
            "set_search",
        );
        match &normalized {
            Some(q) => crate::debug::tapline!(shared.debug_tap, "search", "\"{}\"", q),
            None => crate::debug::tapline!(shared.debug_tap, "search", "cleared"),
        }
        *self.search.write().unwrap() = normalized;
        self.repoint_unless_refused(Change::Clause);
    }

    /// A refused tail shows nothing whatever its query, so it stays where it
    /// is until an order lifts the refusal, and moves once then.
    fn repoint_unless_refused(&self, change: Change) {
        if self.refusal().is_some() {
            self.signal.bump_stats();
        } else {
            self.repoint(change);
        }
    }
}

/// The order-insensitive identity of a filter set.
fn conditions_key(conditions: &[Condition]) -> String {
    let mut query = VistaQuery::new();
    query.conditions = conditions.to_vec();
    query.key()
}
