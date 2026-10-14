//! Relationship traversal from a loaded record handle.
//!
//! The record-level equivalent of `Table::get_ref_from_row`:
//! `launch.get_ref::<LaunchCrew>("launch_crew")` returns the child table scoped
//! to that launch, carrying the foreign-key invariant so inserts conform.
//! `ActiveEntity` and `ActiveRecord` live in `vantage-dataset` and know nothing
//! about `Table`, hence an extension trait here.

use vantage_core::{Result, error};
use vantage_dataset::prelude::{ActiveEntity, ActiveRecord};
use vantage_types::{Entity, InvariantValue, TryIntoRecord};

use crate::{table::Table, traits::table_source::TableSource};

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

        // The entity struct carries no id column, but has-many traversal reads
        // the parent id out of the row.
        let id_field = self.dataset().id_field_name();
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
        let row: &vantage_types::Record<T::Value> = self;
        self.dataset().get_ref_from_row::<E2>(relation, row)
    }
}
