use async_trait::async_trait;
use vantage_types::{Entity, Record};

use crate::{im::ImTable, traits::InsertableValueSet};

#[async_trait]
impl<E> InsertableValueSet for ImTable<E>
where
    E: Entity,
{
    async fn insert_return_id_value(
        &self,
        record: &Record<Self::Value>,
    ) -> crate::traits::Result<Self::Id> {
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

        // Get current table and insert record
        let mut table = self.data_source.get_or_create_table(&self.table_name);
        table.insert(id.clone(), record.clone());

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
    async fn test_insert_return_id_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let mut record = Record::new();
        record.insert(
            "name".to_string(),
            serde_json::Value::String("Alice".to_string()),
        );
        let id = table.insert_return_id_value(&record).await.unwrap();
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn test_insert_return_id_value_with_existing_id() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let mut record = Record::new();
        record.insert(
            "id".to_string(),
            serde_json::Value::String("user-123".to_string()),
        );
        record.insert(
            "name".to_string(),
            serde_json::Value::String("Bob".to_string()),
        );
        let id = table.insert_return_id_value(&record).await.unwrap();
        assert_eq!(id, "user-123");
    }

    #[tokio::test]
    async fn test_insert_return_id_value_with_null_id() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let mut record = Record::new();
        record.insert("id".to_string(), serde_json::Value::Null);
        record.insert(
            "name".to_string(),
            serde_json::Value::String("Charlie".to_string()),
        );
        let id = table.insert_return_id_value(&record).await.unwrap();
        assert!(!id.is_empty());
        assert_ne!(id, "null");
    }
}
