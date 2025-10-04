//! Core ReDB implementation

use redb::{Database, ReadTransaction, WriteTransaction};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use vantage_expressions::protocol::datasource::DataSource;

#[derive(Error, Debug)]
pub enum RedbError {
    #[error("Database error: {0}")]
    Database(#[from] Box<redb::Error>),
    #[error("Database error: {0}")]
    DatabaseError(#[from] Box<redb::DatabaseError>),
    #[error("Transaction error: {0}")]
    Transaction(#[from] Box<redb::TransactionError>),
    #[error("Storage error: {0}")]
    Storage(#[from] Box<redb::StorageError>),
    #[error("Serialization error: {0}")]
    Serialization(#[from] Box<bincode::Error>),
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
        let db = Database::create(path).map_err(|e| RedbError::DatabaseError(Box::new(e)))?;
        Ok(Redb { db: Arc::new(db) })
    }

    /// Open existing RedbDB instance
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, RedbError> {
        let db = Database::open(path).map_err(|e| RedbError::DatabaseError(Box::new(e)))?;
        Ok(Redb { db: Arc::new(db) })
    }

    /// Get reference to underlying database
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Begin read transaction
    pub fn begin_read(&self) -> Result<ReadTransaction, RedbError> {
        self.db
            .begin_read()
            .map_err(|e| RedbError::Transaction(Box::new(e)))
    }

    /// Begin write transaction
    pub fn begin_write(&self) -> Result<WriteTransaction, RedbError> {
        self.db
            .begin_write()
            .map_err(|e| RedbError::Transaction(Box::new(e)))
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
        T: vantage_core::Entity,
    {
        use vantage_dataset::dataset::DataSetError;
        use vantage_expressions::protocol::selectable::Selectable;

        // Use RedbSelect and execute_select approach
        let mut select = crate::RedbSelect::<T>::new();
        select.set_source(table_name, None);
        let json_result = self.execute_select(&select).await;

        // Check for errors in the JSON response
        if let Some(error) = json_result.get("error") {
            return Err(DataSetError::other(format!("ReDB error: {}", error)));
        }

        // Parse the JSON result into Vec<T>
        if let serde_json::Value::Array(records) = json_result {
            let mut results = Vec::new();
            for record in records {
                let entity: T = serde_json::from_value(record).map_err(|e| {
                    DataSetError::other(format!("Failed to deserialize record: {}", e))
                })?;
                results.push(entity);
            }
            Ok(results)
        } else {
            Ok(Vec::new())
        }
    }

    async fn get_table_data_some_as<T>(
        &self,
        table_name: &str,
    ) -> vantage_dataset::dataset::Result<Option<T>>
    where
        T: vantage_core::Entity,
    {
        use vantage_dataset::dataset::DataSetError;
        use vantage_expressions::protocol::selectable::Selectable;

        // Use RedbSelect with limit 1
        let mut select = crate::RedbSelect::<T>::new();
        select.set_source(table_name, None);
        let select = select.with_limit(1);
        let json_result = self.execute_select(&select).await;

        // Check for errors in the JSON response
        if let Some(error) = json_result.get("error") {
            return Err(DataSetError::other(format!("ReDB error: {}", error)));
        }

        // Parse the JSON result into Option<T>
        if let serde_json::Value::Array(records) = json_result {
            if let Some(first_record) = records.first() {
                let entity: T = serde_json::from_value(first_record.clone()).map_err(|e| {
                    DataSetError::other(format!("Failed to deserialize record: {}", e))
                })?;
                Ok(Some(entity))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn get_table_data_values(
        &self,
        _table_name: &str,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>> {
        use vantage_dataset::dataset::DataSetError;

        // ReDB can't retrieve data as generic JSON values because:
        // 1. Data is stored as binary (bincode)
        // 2. We need the concrete type T to deserialize
        // 3. We can't convert arbitrary binary data to JSON
        Err(DataSetError::no_capability(
            "get_table_data_values",
            "ReDB requires specific entity types for data retrieval - use Table<Redb, YourEntity>.get() instead",
        ))
    }
}
