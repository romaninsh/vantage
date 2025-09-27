//! # SurrealDB Table Write Operations
//!
//! This module implements the standard `WritableDataSet` and `InsertableDataSet` traits
//! for SurrealDB tables, providing proper integration with the Vantage dataset ecosystem.

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use vantage_dataset::dataset::{Id, InsertableDataSet, Result, WritableDataSet};
use vantage_table::{Entity, Table};

use super::SurrealTableCore;
use crate::SurrealDB;

#[async_trait]
impl<E> WritableDataSet<E> for Table<SurrealDB, E>
where
    E: Entity + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    /// Insert a record with a specific ID, fails if ID already exists
    async fn insert_id(&self, id: impl Id, record: E) -> Result<()> {
        let id_str = id.into();
        let data = serde_json::to_value(&record).map_err(|e| {
            vantage_dataset::dataset::DataSetError::other(format!("Serialization failed: {}", e))
        })?;

        // Use with_id to create a table filtered to this specific record, then use insert
        let filtered_table = self.clone().with_id(id);

        let client = self.data_source().inner.lock().await;
        client
            .insert(&format!("{}:{}", self.table_name(), id_str), data)
            .await
            .map_err(|e| {
                vantage_dataset::dataset::DataSetError::other(format!(
                    "SurrealDB insert failed: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Replace a record by ID (upsert - creates if missing, replaces if exists)
    async fn replace_id(&self, id: impl Id, record: E) -> Result<()> {
        let id_str = id.into();
        let data = serde_json::to_value(&record).map_err(|e| {
            vantage_dataset::dataset::DataSetError::other(format!("Serialization failed: {}", e))
        })?;

        // Use with_id to create a table filtered to this specific record
        let _filtered_table = self.clone().with_id(&id_str);

        let client = self.data_source().inner.lock().await;
        client
            .update(&format!("{}:{}", self.table_name(), id_str), Some(data))
            .await
            .map_err(|e| {
                vantage_dataset::dataset::DataSetError::other(format!(
                    "SurrealDB replace failed: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Partially update a record by ID using JSON patch, fails if record doesn't exist
    async fn patch_id(&self, id: impl Id, partial: serde_json::Value) -> Result<()> {
        let id_str = id.into();
        let record_id = format!("{}:{}", self.table_name(), id_str);

        // Use with_id to create a table filtered to this specific record
        let _filtered_table = self.clone().with_id(&id_str);

        let client = self.data_source().inner.lock().await;
        client.merge(&record_id, partial).await.map_err(|e| {
            vantage_dataset::dataset::DataSetError::other(format!("SurrealDB patch failed: {}", e))
        })?;

        Ok(())
    }

    /// Delete a record by ID
    async fn delete_id(&self, id: impl Id) -> Result<()> {
        let id_str = id.into();
        let record_id = format!("{}:{}", self.table_name(), id_str);

        // Use with_id to create a table filtered to this specific record
        let _filtered_table = self.clone().with_id(&id_str);

        let client = self.data_source().inner.lock().await;
        client.delete(&record_id).await.map_err(|e| {
            vantage_dataset::dataset::DataSetError::other(format!("SurrealDB delete failed: {}", e))
        })?;

        Ok(())
    }

    /// Update records using a callback that modifies each record in place
    async fn update<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&mut E) + Send + Sync,
    {
        use super::SurrealTableSelectable;

        // Get all records with their IDs
        let records = self.get_with_ids().await?;

        for (id, mut record) in records {
            let original_record = record.clone();
            callback(&mut record);

            // Only update if the record was actually modified
            let original_value = serde_json::to_value(&original_record).map_err(|e| {
                vantage_dataset::dataset::DataSetError::other(format!(
                    "Failed to serialize original record: {}",
                    e
                ))
            })?;
            let new_value = serde_json::to_value(&record).map_err(|e| {
                vantage_dataset::dataset::DataSetError::other(format!(
                    "Failed to serialize modified record: {}",
                    e
                ))
            })?;

            if original_value != new_value {
                self.replace_id(id, record).await?;
            }
        }

        Ok(())
    }

    /// Delete all records in the DataSet
    async fn delete(&self) -> Result<()> {
        use super::SurrealTableSelectable;

        // Get all record IDs
        let records = self.get_with_ids().await?;

        for (id, _) in records {
            self.delete_id(id).await?;
        }

        Ok(())
    }
}

#[async_trait]
impl<E> InsertableDataSet<E> for Table<SurrealDB, E>
where
    E: Entity + Serialize + Send + Sync + 'static,
{
    /// Insert a record and return generated ID
    async fn insert(&self, record: E) -> Result<String> {
        let data = serde_json::to_value(&record).map_err(|e| {
            vantage_dataset::dataset::DataSetError::other(format!("Serialization failed: {}", e))
        })?;

        let client = self.data_source().inner.lock().await;
        let result = client.insert(self.table_name(), data).await.map_err(|e| {
            vantage_dataset::dataset::DataSetError::other(format!("SurrealDB insert failed: {}", e))
        })?;

        // Extract the ID from the result
        if let serde_json::Value::Array(results) = result {
            if let Some(serde_json::Value::Object(obj)) = results.first() {
                if let Some(serde_json::Value::String(id)) = obj.get("id") {
                    // Remove table prefix from ID if present (e.g., "users:123" -> "123")
                    let clean_id = if let Some(colon_pos) = id.find(':') {
                        id[colon_pos + 1..].to_string()
                    } else {
                        id.clone()
                    };
                    return Ok(clean_id);
                }
            }
        }

        Err(vantage_dataset::dataset::DataSetError::other(
            "Failed to extract ID from insert result",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct TestEntity {
        name: String,
        value: i32,
    }

    impl Entity for TestEntity {}

    #[test]
    fn test_writable_api() {
        // This test demonstrates the intended API usage
        // In a real scenario, you'd have a working SurrealDB connection

        // let db = SurrealDB::new(client);
        // let table = Table::new("test", db).into_entity::<TestEntity>();

        // Test insert with ID
        // table.insert_id("test1", TestEntity { name: "Test".to_string(), value: 42 }).await.unwrap();

        // Test replace
        // table.replace_id("test1", TestEntity { name: "Updated".to_string(), value: 100 }).await.unwrap();

        // Test patch
        // let patch = serde_json::json!({"value": 200});
        // table.patch_id("test1", patch).await.unwrap();

        // Test delete
        // table.delete_id("test1").await.unwrap();

        // Test insert with generated ID
        // let id = table.insert(TestEntity { name: "Auto ID".to_string(), value: 300 }).await.unwrap();

        // Test update with callback
        // table.update(|record| record.value += 1).await.unwrap();
    }
}
