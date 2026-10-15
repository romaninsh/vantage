use async_trait::async_trait;

use vantage_core::Result;
use vantage_dataset::prelude::{WritableDataSet, WritableValueSet};
use vantage_types::{Entity, InvariantValue, TryIntoRecord};

use crate::{table::Table, traits::table_source::TableSource};

// Delegates to the value-set twin so invariants are enforced in one place.
#[async_trait]
impl<T, E> WritableDataSet<E> for Table<T, E>
where
    T: TableSource,
    T::Value: InvariantValue,
    E: Entity<T::Value>,
    <E as TryIntoRecord<T::Value>>::Error: std::fmt::Debug,
{
    async fn insert(&self, id: impl Into<Self::Id> + Send, entity: &E) -> Result<E> {
        let record = entity
            .clone()
            .try_into_record()
            .map_err(|e| vantage_core::error!("Failed to serialize entity to record", error = e))?;

        let result_record = self.insert_value(id, &record).await?;

        E::try_from_record(&result_record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))
    }

    async fn replace(&self, id: impl Into<Self::Id> + Send, entity: &E) -> Result<E> {
        let record = entity
            .clone()
            .try_into_record()
            .map_err(|e| vantage_core::error!("Failed to serialize entity to record", error = e))?;

        let result_record = self.replace_value(id, &record).await?;

        E::try_from_record(&result_record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))
    }

    async fn patch(&self, id: impl Into<Self::Id> + Send, partial: &E) -> Result<E> {
        let partial_record = partial
            .clone()
            .try_into_record()
            .map_err(|e| vantage_core::error!("Failed to serialize entity to record", error = e))?;

        let result_record = self.patch_value(id, &partial_record).await?;

        E::try_from_record(&result_record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))
    }
}
