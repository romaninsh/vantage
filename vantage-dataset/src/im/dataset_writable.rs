use async_trait::async_trait;
use vantage_types::{Entity, Record, TryFromRecord, TryIntoRecord};

use crate::{
    im::ImTable,
    traits::{Result, WritableDataSet},
};

#[async_trait]
impl<E> WritableDataSet<E> for ImTable<E>
where
    E: Entity + Clone + Send + Sync,
    <E as TryFromRecord<serde_json::Value>>::Error: std::fmt::Debug,
    <E as TryIntoRecord<serde_json::Value>>::Error: std::fmt::Debug,
{
    async fn insert(&self, id: impl Into<Self::Id> + Send, entity: &E) -> Result<E> {
        let id = id.into();
        self.data_source.with_table_mut(&self.table_name, |table| {
            if let Some(existing_record) = table.get(&id) {
                let mut record_with_id = existing_record.clone();
                record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

                return E::try_from_record(&record_with_id).map_err(|e| {
                    vantage_core::error!("Failed to convert record to entity", error = e)
                });
            }

            // The id lives in the key, not in the stored record.
            let mut record: Record<serde_json::Value> =
                entity.clone().try_into_record().map_err(|e| {
                    vantage_core::error!("Failed to serialize entity to record", error = e)
                })?;
            record.shift_remove("id");

            table.insert(id.clone(), record);
            Ok(entity.clone())
        })
    }

    async fn replace(&self, id: impl Into<Self::Id> + Send, entity: &E) -> Result<E> {
        let id = id.into();
        let mut record: Record<serde_json::Value> = entity
            .clone()
            .try_into_record()
            .map_err(|e| vantage_core::error!("Failed to serialize entity to record", error = e))?;
        record.shift_remove("id");

        self.data_source.with_table_mut(&self.table_name, |table| {
            table.insert(id.clone(), record);
        });

        Ok(entity.clone())
    }

    async fn patch(&self, id: impl Into<Self::Id> + Send, partial: &E) -> Result<E> {
        let id = id.into();
        let partial_record: Record<serde_json::Value> = partial
            .clone()
            .try_into_record()
            .map_err(|e| vantage_core::error!("Failed to serialize entity to record", error = e))?;

        self.data_source.with_table_mut(&self.table_name, |table| {
            let mut existing_record = table
                .get(&id)
                .ok_or_else(|| {
                    vantage_core::error!("Record not found", id = id.clone()).mark_not_found()
                })?
                .clone();

            for (key, value) in partial_record.iter() {
                // The id is the key; a patch can't rename the row.
                if key != "id" {
                    existing_record.insert(key.clone(), value.clone());
                }
            }

            table.insert(id.clone(), existing_record.clone());

            let mut record_with_id = existing_record;
            record_with_id.insert("id".to_string(), serde_json::Value::String(id));

            E::try_from_record(&record_with_id)
                .map_err(|e| vantage_core::error!("Failed to convert record to entity", error = e))
        })
    }
}
