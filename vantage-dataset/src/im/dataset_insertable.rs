use async_trait::async_trait;
use vantage_types::{Entity, Record};

use crate::{im::ImTable, traits::InsertableDataSet};

#[async_trait]
impl<E> InsertableDataSet<E> for ImTable<E>
where
    E: Entity,
{
    async fn insert_return_id(&self, entity: &E) -> crate::traits::Result<Self::Id> {
        // Convert entity to record
        let mut record: Record<serde_json::Value> = entity.clone().into();

        // Extract ID from record if present, otherwise generate random ID
        let id = if let Some(record_id) = record.get("id") {
            if record_id.is_null() {
                self.generate_id()
            } else if let Some(id_str) = record_id.as_str() {
                if id_str.is_empty() {
                    self.generate_id()
                } else {
                    id_str.to_string()
                }
            } else if let Some(id_num) = record_id.as_u64() {
                id_num.to_string()
            } else {
                self.generate_id()
            }
        } else {
            self.generate_id()
        };

        // Remove id from the stored record since it's in the key
        record.shift_remove("id");

        // Get current table and insert record
        let mut table = self.data_source.get_or_create_table(&self.table_name);
        table.insert(id.clone(), record);

        // Update the table in data source
        self.data_source.update_table(&self.table_name, table);

        Ok(id)
    }
}

// Tests are in tests/im_dataset.rs integration tests
