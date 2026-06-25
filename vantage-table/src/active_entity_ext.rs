//! Relationship traversal from a loaded record handle.
//!
//! [`ActiveEntity`] and [`ActiveRecord`] live in `vantage-dataset` and know
//! nothing about `Table`, so this `Table`-aware extension is provided here as a
//! trait. It is the record-level equivalent of `table.get_ref_from_row(...)`:
//! given a launch you loaded with `get_entity` (typed) or `get_value_record`
//! (untyped), `launch.get_ref::<LaunchCrew>("launch_crew")` returns the child
//! table scoped to that launch (and carrying the foreign-key invariant, so
//! inserts conform — see `Table::add_invariant`).
//!
//! `ActiveEntity` wraps a typed struct with no id column, so it serializes the
//! entity and injects its id before traversing. `ActiveRecord` already holds the
//! raw row (including id/foreign-key columns), so its impl forwards directly.

use vantage_core::{Result, error};
use vantage_dataset::prelude::{ActiveEntity, ActiveRecord};
use vantage_types::{Entity, InvariantValue, TryIntoRecord};

use crate::{
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

/// Traverse a relation from a loaded record.
pub trait GetRefExt<T: TableSource, E> {
    /// Return the related set for `relation`, reading the join value out of the
    /// in-memory entity. Errors if the relation is unknown to the table.
    fn get_ref<E2: Entity<T::Value> + 'static>(&self, relation: &str) -> Result<Table<T, E2>>;
}

impl<'a, T, E> GetRefExt<T, E> for ActiveEntity<'a, Table<T, E>, E>
where
    T: TableSource,
    T::Id: Into<T::Value>,
    T::Value: InvariantValue,
    E: Entity<T::Value> + 'static,
    <E as TryIntoRecord<T::Value>>::Error: std::fmt::Debug,
{
    fn get_ref<E2: Entity<T::Value> + 'static>(&self, relation: &str) -> Result<Table<T, E2>> {
        let mut record = self
            .data()
            .clone()
            .try_into_record()
            .map_err(|e| error!("Failed to serialize entity to record", error = e))?;

        // The entity struct carries no id column, but has-many traversal reads the
        // parent id out of the row — inject it from the ActiveEntity's known id.
        let id_field = self
            .dataset()
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());
        record.insert(id_field, self.id().clone().into());

        self.dataset().get_ref_from_row::<E2>(relation, &record)
    }
}

impl<'a, T, E> GetRefExt<T, E> for ActiveRecord<'a, Table<T, E>>
where
    T: TableSource,
    T::Value: InvariantValue,
    E: Entity<T::Value> + 'static,
{
    fn get_ref<E2: Entity<T::Value> + 'static>(&self, relation: &str) -> Result<Table<T, E2>> {
        // The raw row is already in hand (id and foreign keys included), so unlike
        // the `ActiveEntity` impl there is nothing to serialize or inject.
        let row: &vantage_types::Record<T::Value> = self;
        self.dataset().get_ref_from_row::<E2>(relation, row)
    }
}
