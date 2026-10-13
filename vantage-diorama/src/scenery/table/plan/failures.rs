//! The [`LoadFailure`]s a view fails with, each naming the clause it can't
//! answer.

use vantage_core::VantageError;
use vantage_vista::VistaQuery;

use crate::dio::{AugmentedClause, DioInner};
use crate::scenery::table::{Clause, LoadFailure};

/// The failure for a query the master rejected outright, naming the first
/// clause it rejects on its own.
pub(super) fn rejection(dio: &DioInner, query: &VistaQuery, error: &VantageError) -> LoadFailure {
    let failure = LoadFailure::new(dio.master.name(), error.to_string());
    let rejects = |clause: VistaQuery| dio.plan(&clause).is_err();
    if let Some(condition) = query
        .conditions
        .iter()
        .find(|c| rejects(VistaQuery::new().with_condition((*c).clone())))
    {
        return failure.naming(Clause::Filter(condition.column.clone()));
    }
    if let Some((column, dir)) = &query.order
        && rejects(VistaQuery::new().with_order(column, *dir))
    {
        return failure.naming(Clause::Order(column.clone()));
    }
    match &query.search {
        Some(text) if rejects(VistaQuery::new().with_search(text)) => {
            failure.naming(Clause::Search)
        }
        _ => failure,
    }
}

/// The failure for a titles-only view whose query or Dio residual names an
/// augmented column in a condition or the order: the view never hydrates,
/// so the clause never matches.
///
/// A search is the one clause a picker answers without hydrating: it is
/// typed against the titles the picker lists, and matches over them.
pub(super) fn unhydrated(dio: &DioInner, query: &VistaQuery) -> Option<LoadFailure> {
    let failure = LoadFailure::new(
        dio.master.name(),
        "a titles-only view never hydrates augmented columns",
    );
    let named = [query, &dio.residual]
        .into_iter()
        .find_map(|q| match dio.augmented_clause(q) {
            Some(AugmentedClause::Search) | None => None,
            named => named,
        })?;
    Some(failure.naming(match named {
        AugmentedClause::Filter(column) => Clause::Filter(column.to_string()),
        AugmentedClause::Order(column) => Clause::Order(column.to_string()),
        AugmentedClause::Search => Clause::Search,
    }))
}

/// The failure for a residual a paged view has no complete set to evaluate
/// over, naming its first clause.
pub(super) fn unevaluated(dio: &DioInner, residual: &VistaQuery) -> LoadFailure {
    let failure = LoadFailure::new(
        dio.master.name(),
        "the source can't apply this clause, and the view pages without an \
         eager start to evaluate it over the whole set",
    );
    if let Some(condition) = residual.conditions.first() {
        failure.naming(Clause::Filter(condition.column.clone()))
    } else if residual.search.is_some() {
        failure.naming(Clause::Search)
    } else if let Some((column, _)) = &residual.order {
        failure.naming(Clause::Order(column.clone()))
    } else {
        failure
    }
}
