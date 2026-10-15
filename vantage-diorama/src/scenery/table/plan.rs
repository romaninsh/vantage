//! The view's query, and where it goes.
//!
//! A table view's clauses arrive through several setters: the builder's
//! `filter`s, the runtime `set_filters`, search and order.
//! [`TableView::view_query`](super::TableView::view_query) composes them
//! into the one `VistaQuery` its [`SharedRows`] answer and a paged loader
//! receives, and [`ViewPlan`] records where the Dio routes it.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Vista, VistaQuery};

use crate::dio::{DioInner, Plan, admitted_by_dio};

use super::state::SharedRows;

mod failures;
mod serving;

pub(crate) use serving::Serving;

/// A view query and what the Dio's plan for it leaves over. Recomputed
/// whenever the view query changes, so it always describes the query the
/// next load sends.
pub(crate) struct ViewPlan {
    query: VistaQuery,
    has_residual: bool,
    /// What the master refused of `query`, with the Dio's own residual laid
    /// over it.
    residual: VistaQuery,
    /// The master with the clauses it took of `query` applied.
    narrowed: Vista,
}

impl ViewPlan {
    /// Plan `query` against `dio`'s master. Fails when the master rejects a
    /// clause outright.
    pub(crate) fn new(dio: &DioInner, query: VistaQuery) -> Result<Self> {
        let Plan { narrowed, refused } = dio.plan(&query)?;
        Ok(Self {
            has_residual: !refused.is_empty() || dio.residual_narrows(),
            residual: dio.with_own_residual(&query, refused),
            query,
            narrowed,
        })
    }

    /// The view query this plan routes: what the next load sends.
    pub(crate) fn query(&self) -> &VistaQuery {
        &self.query
    }

    /// The master narrowed by the clauses it took, which a count pushed
    /// down to the master reads.
    pub(crate) fn narrowed(&self) -> &Vista {
        &self.narrowed
    }

    /// Whether a window read from the master leaves anything out: a clause
    /// of the view query the master refused or diorama kept (order
    /// included), or a condition or search of the Dio's own residual.
    ///
    /// This is the per-query order decision too: a view without a residual
    /// has its order, if any, applied by the master (`Order` and an
    /// `Orderable` column), so its windows arrive sorted and nothing
    /// re-sorts them.
    pub(crate) fn has_residual(&self) -> bool {
        self.has_residual
    }

    /// Every clause a window read from the master doesn't reflect: what the
    /// master refused of the view query, plus the Dio's own residual.
    ///
    /// A query holds one search: when the view's search and the Dio's are
    /// both left over, this names the view's. Evaluating the residual over a
    /// complete set must therefore always also apply [`admitted_by_dio`],
    /// never `residual().matches` alone.
    pub(crate) fn residual(&self) -> &VistaQuery {
        &self.residual
    }
}

impl SharedRows {
    /// Every clause these rows answer, as one query.
    pub(crate) fn view_query(&self) -> VistaQuery {
        self.query.clone()
    }

    /// The plan for the query, or `None` when the master rejected it
    /// outright. Rows never load a rejected query: their [`Serving`] is
    /// failed instead.
    pub(crate) fn view_plan(&self) -> Option<Arc<ViewPlan>> {
        self.view_plan.read().unwrap().clone()
    }

    /// Plan the query and decide the [`Serving`]. Runs once, as the rows are
    /// built. Returns `true` when the change of serving from the one the
    /// rows started with already reloaded them (see [`SharedRows::enter`]),
    /// so a caller that would reload skips its own.
    pub(crate) fn replan(&self) -> bool {
        let Some(dio) = self.dio_weak.upgrade() else {
            return false;
        };
        let (offered, sugar, planned) = self.plan_query(&dio, self.view_query());
        let next = Serving::decide(self, &dio, &offered, &sugar, planned.as_ref());
        *self.view_plan.write().unwrap() = match planned {
            Ok(plan) => Some(Arc::new(plan)),
            Err(e) => {
                tracing::debug!(
                    target: "vantage_diorama::viewport",
                    table = %dio.master.name(),
                    error = %e,
                    "the master rejected the view query",
                );
                None
            }
        };
        let previous = std::mem::replace(&mut *self.serving.write().unwrap(), next.clone());
        self.enter(&dio, &previous, &next)
    }

    /// Plan `query` as these rows plan their own: the clauses offered to the
    /// master, those on sugar outputs, and the Dio's plan for the offered
    /// ones.
    pub(crate) fn plan_query(
        &self,
        dio: &DioInner,
        query: VistaQuery,
    ) -> (VistaQuery, VistaQuery, Result<ViewPlan>) {
        let (offered, sugar) = self.split_sugar(query);
        let planned = ViewPlan::new(dio, offered.clone());
        (offered, sugar, planned)
    }

    /// Split `query` into what may be offered to the master and the clauses
    /// on sugar outputs, which only the rows this view reads carry.
    fn split_sugar(&self, mut query: VistaQuery) -> (VistaQuery, VistaQuery) {
        let mut sugar = VistaQuery::new();
        if self.sugar_columns.is_empty() {
            return (query, sugar);
        }
        let on_sugar = |column: &str| self.sugar_columns.contains(column);
        let (local, offered) = std::mem::take(&mut query.conditions)
            .into_iter()
            .partition(|c| on_sugar(&c.column));
        query.conditions = offered;
        sugar.conditions = local;
        if query
            .order
            .as_ref()
            .is_some_and(|(column, _)| on_sugar(column))
        {
            sugar.order = query.order.take();
        }
        (query, sugar)
    }

    /// Whether the query narrows the set: a condition or a search. A total
    /// stated under a narrowing query describes the narrowed set, so it is
    /// neither remembered as the table's total nor restored into these rows.
    pub(crate) fn view_narrows(&self) -> bool {
        !self.query.conditions.is_empty() || self.query.search.is_some()
    }
}

/// Whether `record` belongs in a view over the complete set: it satisfies
/// every clause of the view's `query` and the Dio's residual.
///
/// The whole view query is evaluated, not only the clauses the master
/// refused: the cache holds the Dio's rows, which no view clause narrowed.
/// The Dio's residual is applied through [`admitted_by_dio`], because a
/// query holds one search and the view's may stand beside the Dio's.
pub(crate) fn admitted_by_view(
    dio: &DioInner,
    query: &VistaQuery,
    record: &Record<CborValue>,
) -> bool {
    let view = match query.matches(record, &dio.metadata) {
        Ok(admitted) => admitted,
        Err(e) => {
            tracing::warn!(
                target: "vantage_diorama::filter",
                table = %dio.master.name(),
                error = %e,
                "the view query can't be evaluated; the row is left out",
            );
            false
        }
    };
    view && admitted_by_dio(dio, record)
}
