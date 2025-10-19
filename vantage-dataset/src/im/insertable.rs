use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    dataset::{Importable, InsertableDataSet, ReadableDataSet, Result},
    im::ImTable,
};
use vantage_core::util::error::Context;

#[async_trait]
impl<E> InsertableDataSet<E> for ImTable<E>
where
    E: Serialize + DeserializeOwned + Send + Sync,
{
    async fn insert(&self, record: E) -> Result<Option<String>> {
        // Serialize record to JSON
        let mut value =
            serde_json::to_value(record).context("Failed to serialize record to JSON")?;

        // Extract ID from record if present, otherwise generate random ID
        let id = if let Some(record_id) = value.get("id") {
            if record_id.is_null() {
                self.generate_id()
            } else if let Some(id_str) = record_id.as_str() {
                id_str.to_string()
            } else if let Some(id_num) = record_id.as_u64() {
                id_num.to_string()
            } else {
                self.generate_id()
            }
        } else {
            self.generate_id()
        };

        // Remove id from the stored record since it's in the key
        if let serde_json::Value::Object(ref mut map) = value {
            map.remove("id");
        }

        // Get current table and insert record
        let mut table = self.data_source.get_or_create_table(&self.table_name);
        table.insert(id.clone(), value);

        // Update the table in data source
        self.data_source.update_table(&self.table_name, table);

        Ok(Some(id))
    }
}

#[async_trait]
impl<T> Importable<T> for ImTable<T>
where
    T: Serialize + DeserializeOwned + Send + Sync,
{
    async fn import<D>(&mut self, source: D) -> Result<()>
    where
        D: ReadableDataSet<T> + Send,
    {
        let records = source.get().await?;
        for record in records {
            self.insert(record).await?;
        }
        Ok(())
    }
}
