use async_trait::async_trait;

use vantage_core::Result;
use vantage_dataset::prelude::{InsertableDataSet, InsertableValueSet};
use vantage_types::{Entity, InvariantValue, TryIntoRecord};

use crate::{table::Table, traits::table_source::TableSource};

// Delegates to the value-set twin so invariants are enforced in one place.
#[async_trait]
impl<T, E> InsertableDataSet<E> for Table<T, E>
where
    T: TableSource,
    T::Value: InvariantValue,
    E: Entity<T::Value>,
    <E as TryIntoRecord<T::Value>>::Error: std::fmt::Debug,
{
    async fn insert_return_id(&self, entity: &E) -> Result<Self::Id> {
        let record = entity
            .clone()
            .try_into_record()
            .map_err(|e| vantage_core::error!("Failed to serialize entity to record", error = e))?;

        self.insert_return_id_value(&record).await
    }
}
