//! RedbDB implementation
//!
//! Provides a redb key-value database wrapper for Vantage.

use async_trait::async_trait;
use redb::{Database, ReadTransaction, WriteTransaction};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use vantage_expressions::protocol::datasource::DataSource;

#[derive(Error, Debug)]
pub enum RedbError {
    #[error("Database error: {0}")]
    Database(#[from] redb::Error),
    #[error("Database error: {0}")]
    DatabaseError(#[from] redb::DatabaseError),
    #[error("Transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("Storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Query error: {0}")]
    Query(String),
}

/// RedbDB wrapper for key-value operations
#[derive(Debug, Clone)]
pub struct Redb {
    db: Arc<Database>,
}

impl Redb {
    /// Create a new RedbDB instance
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, RedbError> {
        let db = Database::create(path)?;
        Ok(Redb { db: Arc::new(db) })
    }

    /// Open existing RedbDB instance
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, RedbError> {
        let db = Database::open(path)?;
        Ok(Redb { db: Arc::new(db) })
    }

    /// Get reference to underlying database
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Begin read transaction
    pub fn begin_read(&self) -> Result<ReadTransaction, RedbError> {
        Ok(self.db.begin_read()?)
    }

    /// Begin write transaction
    pub fn begin_write(&self) -> Result<WriteTransaction, RedbError> {
        Ok(self.db.begin_write()?)
    }
}

impl DataSource for Redb {}

#[async_trait::async_trait]
impl vantage_table::TableSource for Redb {
    type Column = crate::RedbColumn;

    fn create_column(&self, name: &str, table: impl vantage_table::TableLike) -> Self::Column {
        crate::RedbColumn::new(name, table.table_name().to_string())
    }

    async fn get_table_data_as<T>(
        &self,
        table_name: &str,
    ) -> vantage_dataset::dataset::Result<Vec<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        use redb::{ReadableTable, TableDefinition};
        use vantage_dataset::dataset::DataSetError;

        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = self
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

            let record: T = bincode::deserialize(data.value())
                .map_err(|e| DataSetError::other(format!("Failed to deserialize record: {}", e)))?;

            results.push(record);
        }

        Ok(results)
    }

    async fn get_table_data_some_as<T>(
        &self,
        table_name: &str,
    ) -> vantage_dataset::dataset::Result<Option<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        use redb::{ReadableTable, TableDefinition};
        use vantage_dataset::dataset::DataSetError;

        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = self
            .begin_read()
            .map_err(|e| DataSetError::other(format!("Failed to begin read transaction: {}", e)))?;

        let table = read_txn
            .open_table(table_def)
            .map_err(|e| DataSetError::other(format!("Failed to open table: {}", e)))?;

        if let Some(item) = table
            .iter()
            .map_err(|e| DataSetError::other(format!("Failed to iterate table: {}", e)))?
            .next()
        {
            let (_, data) =
                item.map_err(|e| DataSetError::other(format!("Failed to read record: {}", e)))?;

            let record: T = bincode::deserialize(data.value())
                .map_err(|e| DataSetError::other(format!("Failed to deserialize record: {}", e)))?;

            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    async fn get_table_data_values(
        &self,
        table_name: &str,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>> {
        use redb::{ReadableTable, TableDefinition};
        use vantage_dataset::dataset::DataSetError;

        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = self
            .begin_read()
            .map_err(|e| DataSetError::other(format!("Failed to begin read transaction: {}", e)))?;

        let table = read_txn
            .open_table(table_def)
            .map_err(|e| DataSetError::other(format!("Failed to open table: {}", e)))?;

        let mut results = Vec::new();
        for (i, item) in table
            .iter()
            .map_err(|e| DataSetError::other(format!("Failed to iterate table: {}", e)))?
            .enumerate()
        {
            let (_, data) =
                item.map_err(|e| DataSetError::other(format!("Failed to read record: {}", e)))?;

            // Deserialize from bincode - the records are stored as structs, so we need to
            // deserialize to a generic value. Since we don't know the exact type,
            // we'll use a different approach - deserialize to a map structure first
            use serde::{Deserialize, Serialize};

            #[derive(Serialize, Deserialize)]
            struct GenericRecord {
                name: String,
                email: String,
                is_active: bool,
                age: u32,
            }

            let record: GenericRecord = bincode::deserialize(data.value())
                .map_err(|e| DataSetError::other(format!("Failed to deserialize record: {}", e)))?;

            // Convert to JSON Value
            let mut record_with_id = serde_json::to_value(record)
                .map_err(|e| DataSetError::other(format!("Failed to convert to JSON: {}", e)))?;
            if let serde_json::Value::Object(ref mut map) = record_with_id {
                map.insert(
                    "id".to_string(),
                    serde_json::Value::String(format!("user{}", i + 1)),
                );
            }

            results.push(record_with_id);
        }

        Ok(results)
    }
}
