//! Which clauses can only be judged over rows that carry their augmented
//! values.
//!
//! An augmented column exists only in the cache, and only once a row has
//! been through the detail pass. A clause naming one, judged over a row that
//! hasn't, decides on a gap: `Eq` drops the row and `Ne` admits it, both
//! silently. Every path that judges cached rows asks
//! [`DioInner::names_augmented`] first and hydrates the rows when it holds.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::VistaQuery;

use super::{DetailState, Dio, DioInner};
use crate::lens::CacheStatus;

/// The clause [`DioInner::augmented_clause`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AugmentedClause<'q> {
    /// A condition on this column.
    Filter(&'q str),
    /// The order, on this column.
    Order(&'q str),
    /// The search, which reads every cell.
    Search,
}

impl DioInner {
    /// Whether `column`, or the column a dotted path starts at, is one an
    /// augmentation declares it fills. A lift-all augmentation declares none.
    pub(crate) fn is_declared_augmented(&self, column: &str) -> bool {
        let root = column.split('.').next().unwrap_or(column);
        self.is_augmented_column(column) || self.is_augmented_column(root)
    }

    /// Whether `column`, or the column a dotted path starts at, may be filled
    /// by augmentation. A lift-all augmentation declares no columns, so on
    /// such a Dio any column may be.
    pub(crate) fn may_be_augmented(&self, column: &str) -> bool {
        self.has_dio_augment()
            && (self.augmented_columns.is_empty() || self.is_declared_augmented(column))
    }

    /// The first clause of `query` that can only be judged over rows carrying
    /// their augmented values: a condition, then the order, on a column that
    /// [`may_be_augmented`](Self::may_be_augmented), then a search on an
    /// augmented Dio. `None` on a Dio without augmentations.
    pub(crate) fn augmented_clause<'q>(
        &self,
        query: &'q VistaQuery,
    ) -> Option<AugmentedClause<'q>> {
        if !self.has_dio_augment() {
            return None;
        }
        if let Some(condition) = query
            .conditions
            .iter()
            .find(|c| self.may_be_augmented(&c.column))
        {
            return Some(AugmentedClause::Filter(&condition.column));
        }
        if let Some((column, _)) = &query.order
            && self.may_be_augmented(column)
        {
            return Some(AugmentedClause::Order(column));
        }
        query.search.as_ref().map(|_| AugmentedClause::Search)
    }

    /// Whether judging `query` together with this Dio's residual over cached
    /// rows needs their augmented values. Pass an empty query to ask about
    /// the residual alone.
    pub(crate) fn names_augmented(&self, query: &VistaQuery) -> bool {
        self.augmented_clause(query).is_some() || self.augmented_clause(&self.residual).is_some()
    }

    /// `query` without the clauses a row lacking augmented values can't
    /// carry: conditions on a declared augmented column (or a dotted path
    /// under one), and the search on an augmented Dio. This judges a row
    /// that hasn't been through the detail pass and never will be here: a
    /// change the master pushed, or a record being written to it. A lift-all
    /// augmentation declares no columns, so its conditions all stay.
    pub(crate) fn without_augmented(&self, query: &VistaQuery) -> VistaQuery {
        let mut kept = query.clone();
        kept.conditions
            .retain(|c| !self.is_declared_augmented(&c.column));
        if self.has_dio_augment() {
            kept.search = None;
        }
        kept
    }

    /// `row`, read from the master on a cache miss, with this Dio's
    /// augmentations run over it in memory, as the detail pass runs them
    /// over a cached row. Nothing is cached. A failed fetch is the error.
    pub(crate) async fn augment_in_memory(
        &self,
        mut row: Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        for aug in self.augmentations.iter().flat_map(|augs| augs.iter()) {
            aug.augment_row(self.master.name(), &self.id_column, &mut row)
                .await?;
        }
        Ok(row)
    }

    /// Whether the last detail fetch for `id` failed: the row carries no
    /// augmented values a clause could judge.
    pub(crate) fn detail_failed(&self, id: &str) -> bool {
        self.detail_state(id) == Some(DetailState::Failed)
    }

    /// Whether the cached row `id` still waits for the detail pass before
    /// a clause on an augmented value can judge it. A row the pass settled
    /// with no detail record waits no longer: it really has no value, and
    /// is judged as it is. A row the pass failed on waits no longer either,
    /// but [`detail_failed`](Self::detail_failed) keeps it from being
    /// judged.
    ///
    /// A lift-all augmentation has no columns to find a gap in, so the
    /// status decides. A row an eager start copied is stored `Complete`
    /// without a detail pass, and is judged as it is.
    pub(crate) fn awaits_hydration(
        &self,
        id: &str,
        row: &Record<CborValue>,
        status: CacheStatus,
    ) -> bool {
        if matches!(
            self.detail_state(id),
            Some(DetailState::Empty | DetailState::Failed)
        ) {
            return false;
        }
        if self.augmented_columns.is_empty() {
            return status != CacheStatus::Complete;
        }
        super::augment_passes::has_augment_gap(row, &self.augmented_columns)
    }

    /// The cached row `id` read through `reader`, hydrated first when
    /// `judged_by` or this Dio's residual names an augmented value, so
    /// neither judges a gap. `None` when the cache doesn't hold it. A failed
    /// hydration is the error.
    pub(crate) async fn read_hydrated(
        self: &Arc<Self>,
        reader: &dyn crate::lens::CacheTable,
        id: &str,
        judged_by: &VistaQuery,
    ) -> Result<Option<Record<CborValue>>> {
        let Some(row) = reader.get_value(id).await? else {
            return Ok(None);
        };
        if !self.names_augmented(judged_by) {
            return Ok(Some(row));
        }
        let mut rows = IndexMap::from([(id.to_string(), row)]);
        self.hydrate(&mut rows).await?;
        // The hydrated row is in the cache; a sugared reader adds its
        // outputs over it.
        reader.get_value(id).await
    }

    /// Hydrate every row of `rows` still missing its augmented values,
    /// blocking until the detail pass settles them, and swap the hydrated
    /// rows in. A no-op on a Dio without augmentations.
    pub(crate) async fn hydrate(
        self: &Arc<Self>,
        rows: &mut IndexMap<String, Record<CborValue>>,
    ) -> Result<()> {
        if !self.has_dio_augment() {
            return Ok(());
        }
        let dio = Dio {
            inner: self.clone(),
        };
        super::augment_passes::hydrate_gaps(&dio, rows).await
    }
}
