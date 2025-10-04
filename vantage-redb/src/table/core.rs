//! # Redb Table Core Operations
//!
//! This module provides the core trait implementations for Table<RedbDB, E>.
//! Uses extension traits to avoid orphan rule issues.

use async_trait::async_trait;
use redb::{ReadableTable, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use vantage_dataset::dataset::{DataSetError, Id, ReadableDataSet, Result};
use vantage_table::{Entity, Table};

use crate::Redb;

/// Core trait for Redb table operations that other traits can build upon
#[async_trait]
pub trait RedbTableCore<E: Entity> {
    /// Get all records from the table
    async fn get(&self) -> Result<Vec<E>>;

    /// Get first record from the table
    async fn get_some(&self) -> Result<Option<E>>;

    /// Get record by ID
    async fn get_id(&self, id: impl Id) -> Result<E>;

    /// Generic methods for any deserializable type
    async fn get_as<T>(&self) -> Result<Vec<T>>
    where
        T: DeserializeOwned;

    async fn get_some_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned;

    /// Get data as serde_json::Value for generic operations like indexing
    async fn get_values(&self) -> Result<Vec<serde_json::Value>>;
}

#[async_trait]
impl<E: Entity + Serialize + DeserializeOwned + Send + Sync> RedbTableCore<E> for Table<Redb, E> {
    async fn get(&self) -> Result<Vec<E>> {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(self.table_name());
        let read_txn = self
            .data_source()
            .begin_read()
            .map_err(|e| DataSetError::other(format!("Failed to begin read transaction: {}", e)))?;

        let table = read_txn
            .open_table(table_def)
            .map_err(|e| DataSetError::other(format!("Failed to open table: {}", e)))?;

        let mut results = Vec::new();

        for item in table
            .iter()
            .map_err(|e| DataSetError::other(format!("Failed to iterate table: {}", e)))?
        {
            let (_, data) =
                item.map_err(|e| DataSetError::other(format!("Failed to read record: {}", e)))?;

            let record: E = bincode::deserialize(data.value())
                .map_err(|e| DataSetError::other(format!("Failed to deserialize record: {}", e)))?;

            results.push(record);
        }

        Ok(results)
    }

    async fn get_some(&self) -> Result<Option<E>> {
        todo!("Implement get_some for redb table")
    }

    async fn get_id(&self, _id: impl Id) -> Result<E> {
        todo!("Implement get_id for redb table")
    }

    async fn get_as<T>(&self) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        todo!("Implement get_as for redb table")
    }

    async fn get_some_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        todo!("Implement get_some_as for redb table")
    }

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        let records = RedbTableCore::get(self).await?;
        let values: Result<Vec<serde_json::Value>> = records
            .into_iter()
            .enumerate()
            .map(|(i, record)| {
                // Serialize to JSON and add id field
                let mut value = serde_json::to_value(&record).map_err(|e| {
                    DataSetError::other(format!("Failed to serialize record: {}", e))
                })?;

                if let serde_json::Value::Object(ref mut map) = value {
                    map.insert(
                        "id".to_string(),
                        serde_json::Value::String(format!("user{}", i + 1)),
                    );
                }

                Ok(value)
            })
            .collect();

        values
    }
}
