use async_trait::async_trait;
use serde::Serialize;
use vantage_core::Entity;

use crate::{
    dataset::{InsertableDataSet, Result},
    im::ImTable,
};
use vantage_core::util::error::Context;

#[async_trait]
impl<E> InsertableDataSet<E> for ImTable<E>
where
    E: Entity + Serialize + Send + Sync,
{
    async fn insert_return_id(&self, record: E) -> Result<Self::Id> {
        // Serialize record to JSON
        let mut value =
            serde_json::to_value(record).context("Failed to serialize record to JSON")?;

        // Extract ID from record if present, otherwise generate random ID
        let id = if let Some(record_id) = value.get("id") {
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
        if let serde_json::Value::Object(ref mut map) = value {
            map.remove("id");
        }

        // Get current table and insert record
        let mut table = self.data_source.get_or_create_table(&self.table_name);
        table.insert(id.clone(), value);

        // Update the table in data source
        self.data_source.update_table(&self.table_name, table);

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im::ImDataSource;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct User {
        id: Option<String>,
        name: String,
    }

    #[tokio::test]
    async fn test_insert_return_id() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let user = User {
            id: None,
            name: "Alice".to_string(),
        };
        let id = table.insert_return_id(user).await.unwrap();
        assert!(!id.is_empty());
    }
}
