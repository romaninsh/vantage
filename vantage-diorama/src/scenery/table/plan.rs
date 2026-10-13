//! The view's query, and where it goes.
//!
//! A table scenery's clauses arrive through several setters: the builder's
//! `where_eq` and `where_op`, the filter chips, the operator terms, search and
//! sort. [`TableSceneryState::view_query`] composes them into the one
//! `VistaQuery` a paged loader receives, and [`ViewPlan`] records where the
//! Dio routes it.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Condition, FilterOp, VistaQuery};

use crate::dio::{DioInner, Plan, admitted_by_dio};

use super::state::TableSceneryState;

mod failures;
mod serving;

pub(crate) use serving::Serving;

/// A view query and the Dio's plan for it. Recomputed whenever the view query
/// changes, so it always describes the query the next load sends.
pub(crate) struct ViewPlan {
    query: VistaQuery,
    plan: Plan,
    /// `plan.refused` with the Dio's own residual laid over it.
    residual: VistaQuery,
    /// Whether the Dio's own residual narrows its rows.
    dio_narrows: bool,
}

impl ViewPlan {
    /// Plan `query` against `dio`'s master. Fails when the master rejects a
    /// clause outright.
    pub(crate) fn new(dio: &DioInner, query: VistaQuery) -> Result<Self> {
        let plan = dio.plan(&query)?;
        Ok(Self {
            residual: dio.with_own_residual(&query, plan.refused.clone()),
            dio_narrows: dio.residual_narrows(),
            query,
            plan,
        })
    }

    /// The view query this plan routes: what the next load sends.
    pub(crate) fn query(&self) -> &VistaQuery {
        &self.query
    }

    /// The master narrowed by every clause it took.
    pub(crate) fn narrowed(&self) -> &vantage_vista::Vista {
        &self.plan.narrowed
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
        !self.plan.refused.is_empty() || self.dio_narrows
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

impl TableSceneryState {
    /// Every clause of this view as one query: the builder's conditions, the
    /// filter chips, the operator terms, search and sort. Built fresh on each
    /// call, because two sceneries over one Dio load concurrently and must
    /// never see each other's narrowing.
    pub(crate) fn view_query(&self) -> VistaQuery {
        let terms = self.ui_terms.read().unwrap().clone();
        self.query_with_terms(&terms)
    }

    /// [`view_query`](Self::view_query) with `terms` standing in for the
    /// view's operator terms.
    pub(crate) fn query_with_terms(&self, terms: &[super::OpCondition]) -> VistaQuery {
        let eq = |(column, value): &(String, CborValue)| {
            Condition::new(column, FilterOp::Eq, value.clone())
        };
        let op =
            |term: &super::OpCondition| Condition::new(&term.column, term.op, term.value.clone());
        let mut query = VistaQuery::new();
        query
            .conditions
            .extend(self.conditions.read().unwrap().iter().map(eq));
        query
            .conditions
            .extend(self.op_conditions.read().unwrap().iter().map(op));
        query
            .conditions
            .extend(self.ui_filters.read().unwrap().iter().map(eq));
        query.conditions.extend(terms.iter().map(op));
        query.search = self.search.read().unwrap().clone();
        query.order = self
            .sort
            .read()
            .unwrap()
            .as_ref()
            .map(|(column, dir)| (column.clone(), dir.to_vista()));
        query
    }

    /// The plan for the current view query, or `None` when the master
    /// rejected it outright. A paged view never loads a rejected query: its
    /// [`Serving`] is failed instead.
    pub(crate) fn view_plan(&self) -> Option<Arc<ViewPlan>> {
        self.view_plan.read().unwrap().clone()
    }

    /// Recompute the plan and the [`Serving`] from the current view query.
    /// Every setter that changes a clause calls this after writing it, and
    /// skips its own reload when this returns `true`: the change of serving
    /// already reloaded the view (see [`TableSceneryState::enter`]).
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

    /// Plan `query` as this view plans its own: the clauses offered to the
    /// master, those on sugar outputs, and the Dio's plan for the offered
    /// ones.
    pub(super) fn plan_query(
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

    /// Whether the view query narrows the set: a condition or a search. A
    /// total stated under a narrowing query describes the narrowed set, so it
    /// is neither remembered as the table's total nor restored into this view.
    pub(crate) fn view_narrows(&self) -> bool {
        let view = self.view_query();
        !view.conditions.is_empty() || view.search.is_some()
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

#[cfg(test)]
mod tests;
