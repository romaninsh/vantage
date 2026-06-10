use async_trait::async_trait;
use vantage_types::{Entity, Record, TryFromRecord};

use crate::{
    im::ImTable,
    traits::{Result, WritableDataSet},
};

#[async_trait]
impl<E> WritableDataSet<E> for ImTable<E>
where
    E: Entity + Clone + Send + Sync,
    <E as TryFromRecord<serde_json::Value>>::Error: std::fmt::Debug,
{
    async fn insert(&self, id: impl Into<Self::Id> + Send, entity: &E) -> Result<E> {
        let id = id.into();
        self.data_source.with_table_mut(&self.table_name, |table| {
            // Check if record already exists (idempotent behavior)
            if let Some(existing_record) = table.get(&id) {
                // Return existing entity; add the id field back for conversion
                let mut record_with_id = existing_record.clone();
                record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

                return E::try_from_record(&record_with_id).map_err(|e| {
                    vantage_core::util::error::vantage_error!(
                        "Failed to convert record to entity: {:?}",
                        e
                    )
                });
            }

            // Convert entity to record for storage (remove id field since it's in the key)
            let mut record: Record<serde_json::Value> = entity.clone().into_record();
            record.shift_remove("id");

            table.insert(id.clone(), record);
            Ok(entity.clone())
        })
    }

    async fn replace(&self, id: impl Into<Self::Id> + Send, entity: &E) -> Result<E> {
        let id = id.into();
        // Convert entity to record for storage (remove id field since it's in the key)
        let mut record: Record<serde_json::Value> = entity.clone().into_record();
        record.shift_remove("id");

        self.data_source.with_table_mut(&self.table_name, |table| {
            table.insert(id.clone(), record);
        });

        Ok(entity.clone())
    }

    async fn patch(&self, id: impl Into<Self::Id> + Send, partial: &E) -> Result<E> {
        let id = id.into();
        // Convert partial entity to record
        let partial_record: Record<serde_json::Value> = partial.clone().into_record();

        self.data_source.with_table_mut(&self.table_name, |table| {
            // Check if record exists
            let mut existing_record = table
                .get(&id)
                .ok_or_else(|| {
                    vantage_core::util::error::vantage_error!("Record with id '{}' not found", id)
                })?
                .clone();

            // Merge the partial fields into the existing record
            for (key, value) in partial_record.iter() {
                if key != "id" {
                    // Don't allow patching the id field
                    existing_record.insert(key.clone(), value.clone());
                }
            }

            table.insert(id.clone(), existing_record.clone());

            // Return the merged entity
            let mut record_with_id = existing_record;
            record_with_id.insert("id".to_string(), serde_json::Value::String(id));

            E::try_from_record(&record_with_id).map_err(|e| {
                vantage_core::util::error::vantage_error!(
                    "Failed to convert record to entity: {:?}",
                    e
                )
            })
        })
    }
}

// Tests are in tests/im_dataset.rs integration tests
