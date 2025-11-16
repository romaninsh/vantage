use async_trait::async_trait;
use vantage_types::{Entity, Record};

use crate::{
    im::ImTable,
    traits::{Result, WritableDataSet},
};

#[async_trait]
impl<E> WritableDataSet<E> for ImTable<E>
where
    E: Entity + Clone + Send + Sync,
    <E as TryFrom<Record<serde_json::Value>>>::Error: std::fmt::Debug,
{
    async fn insert(&self, id: &Self::Id, entity: &E) -> Result<E> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if record already exists (idempotent behavior)
        if let Some(existing_record) = table.get(id) {
            // Return existing entity
            let mut record_with_id = existing_record.clone();
            record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

            let existing_entity = E::try_from(record_with_id).map_err(|e| {
                vantage_core::util::error::vantage_error!(
                    "Failed to convert record to entity: {:?}",
                    e
                )
            })?;
            return Ok(existing_entity);
        }

        // Convert entity to record for storage (remove id field since it's in the key)
        let mut record: Record<serde_json::Value> = entity.clone().into();
        record.shift_remove("id");

        table.insert(id.clone(), record);
        self.data_source.update_table(&self.table_name, table);

        Ok(entity.clone())
    }

    async fn replace(&self, id: &Self::Id, entity: &E) -> Result<E> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Convert entity to record for storage (remove id field since it's in the key)
        let mut record: Record<serde_json::Value> = entity.clone().into();
        record.shift_remove("id");

        table.insert(id.clone(), record);
        self.data_source.update_table(&self.table_name, table);

        Ok(entity.clone())
    }

    async fn patch(&self, id: &Self::Id, partial: &E) -> Result<E> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if record exists
        let mut existing_record = table
            .get(id)
            .ok_or_else(|| {
                vantage_core::util::error::vantage_error!("Record with id '{}' not found", id)
            })?
            .clone();

        // Convert partial entity to record
        let partial_record: Record<serde_json::Value> = partial.clone().into();

        // Merge the partial fields into the existing record
        for (key, value) in partial_record.iter() {
            if key != "id" {
                // Don't allow patching the id field
                existing_record.insert(key.clone(), value.clone());
            }
        }

        table.insert(id.clone(), existing_record.clone());
        self.data_source.update_table(&self.table_name, table);

        // Return the merged entity
        let mut record_with_id = existing_record;
        record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

        let merged_entity = E::try_from(record_with_id).map_err(|e| {
            vantage_core::util::error::vantage_error!("Failed to convert record to entity: {:?}", e)
        })?;
        Ok(merged_entity)
    }

    async fn delete(&self, id: &Self::Id) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Delete is idempotent - success even if record doesn't exist
        table.shift_remove(id);

        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);
        table.clear();
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }
}

// Tests are in tests/im_dataset.rs integration tests
