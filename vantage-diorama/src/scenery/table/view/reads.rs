//! What a view reads: its rows, its clauses as one query, and its stats.

use std::sync::Arc;

use vantage_vista::{Condition, VistaQuery};

use crate::scenery::table::cap::Cap;
use crate::scenery::table::state::SharedRows;
use crate::scenery::table::view_stats::{clamp_range, view_filters};
use crate::scenery::table::{LoadFailure, LoadState, ViewState, ViewStats, compute_load_state};

use super::{Placing, TableView, ViewHook, compose};

impl TableView {
    /// The rows the view reads now.
    pub(crate) fn shared(&self) -> Arc<SharedRows> {
        self.shared.read().unwrap().clone()
    }

    /// A hook for attaching this view to rows.
    pub(crate) fn hook(&self) -> ViewHook {
        ViewHook::new(&self.signal, self.ticket.clone())
    }

    pub(crate) fn cap(&self) -> Option<Cap> {
        *self.cap.read().unwrap()
    }

    /// Every clause of this view as one query: the builder's filters, the
    /// runtime filters, search and the order its rows load in.
    pub(crate) fn view_query(&self) -> VistaQuery {
        self.query_with_filters(&self.filters.read().unwrap())
    }

    /// [`view_query`](Self::view_query) with `filters` standing in for the
    /// view's runtime filters.
    pub(crate) fn query_with_filters(&self, filters: &[Condition]) -> VistaQuery {
        let order = self.order.read().unwrap().clone();
        compose(
            &self.base,
            filters,
            self.search.read().unwrap().clone(),
            self.cap()
                .map_or(order.clone(), |cap| cap.load_order(order)),
        )
    }

    /// The columns this view declared it shows (its demand); `None` demands
    /// everything.
    pub(crate) fn demanded_columns(&self) -> Option<Vec<String>> {
        self.demand.clone()
    }

    /// The view's refusal, when it shows no rows whatever its rows hold.
    pub(crate) fn refusal(&self) -> Option<LoadFailure> {
        self.refused.read().unwrap().clone()
    }

    /// The rows' snapshot with this view's shown range (clamped to the rows
    /// it has), its declared order, its own filters and its cap laid over it.
    pub(crate) fn view_stats(&self) -> ViewStats {
        let shared = self.shared();
        let mut stats = shared.stats.snapshot();
        stats.showing = clamp_range(self.shown.read().unwrap().clone(), shared.row_count());
        stats.order = self.order.read().unwrap().clone();
        stats.filters = view_filters(&self.query_filters(), &self.filters.read().unwrap());
        let nothing = ViewStats {
            loaded: 0,
            showing: None,
            has_more: false,
            ..stats.clone()
        };
        if self.refusal().is_some() {
            return ViewStats {
                total: Some(0),
                total_exact: true,
                state: ViewState::Failed,
                ..nothing
            };
        }
        match self.placing(&shared) {
            Placing::Whole => stats,
            Placing::Capped(cap, _) => stats.capped(cap.n()),
            // As `load_state`: loading, unless the rows failed their query.
            Placing::Unplaced(_) => ViewStats {
                total: None,
                total_exact: false,
                state: match compute_load_state(&shared) {
                    LoadState::Failed(_) => ViewState::Failed,
                    _ => ViewState::Loading,
                },
                ..nothing
            },
        }
    }

    /// The conditions that are part of what the view is: the Dio's residual
    /// ones, then the builder's.
    fn query_filters(&self) -> Vec<Condition> {
        let residual = self
            .dio_weak
            .upgrade()
            .map(|dio| dio.residual.conditions.clone())
            .unwrap_or_default();
        residual
            .into_iter()
            .chain(self.base.iter().cloned())
            .collect()
    }
}
