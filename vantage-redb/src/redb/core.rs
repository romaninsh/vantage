//! Core ReDB implementation

use redb::{Database, ReadTransaction, ReadableTable, TableDefinition, WriteTransaction};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use vantage_expressions::protocol::datasource::DataSource;
use vantage_table::{ColumnLike, Table};

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
    #[error("Table error: {0}")]
    Table(#[from] Box<redb::TableError>),
    #[error("Commit error: {0}")]
    Commit(#[from] Box<redb::CommitError>),
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

impl Redb {
    /// Rebuild index table for a specific column by deleting and recreating from scratch
    pub async fn redb_rebuild_index<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        column_name: &str,
    ) -> Result<(), RedbError>
    where
        E: vantage_core::Entity + serde::Serialize + for<'de> serde::Deserialize<'de>,
    {
        let table_name = table.table_name();
        let index_table_name = format!("{}_by_{}", table_name, column_name);

        // Begin write transaction
        let write_txn = self.begin_write()?;

        {
            // Delete existing index table by dropping it
            let index_table_def: TableDefinition<&str, &str> =
                TableDefinition::new(&index_table_name);

            // Drop the index table if it exists
            if write_txn.open_table(index_table_def).is_ok() {
                write_txn
                    .delete_table(index_table_def)
                    .map_err(|e| RedbError::Table(Box::new(e)))?;
            }

            // Recreate index table
            let mut new_index_table = write_txn
                .open_table(index_table_def)
                .map_err(|e| RedbError::Table(Box::new(e)))?;

            // Read all records from main table
            let main_table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
            let main_table = write_txn
                .open_table(main_table_def)
                .map_err(|e| RedbError::Table(Box::new(e)))?;

            // Walk through all records and rebuild index
            for record_result in main_table
                .iter()
                .map_err(|e| RedbError::Storage(Box::new(e)))?
            {
                let (record_id, record_data) =
                    record_result.map_err(|e| RedbError::Storage(Box::new(e)))?;

                // Deserialize record
                let record: E = bincode::deserialize(record_data.value())
                    .map_err(|e| RedbError::Serialization(Box::new(e)))?;

                // Get column value as JSON to extract string value
                let record_json = serde_json::to_value(&record)
                    .map_err(|e| RedbError::Query(format!("JSON serialization failed: {}", e)))?;

                if let Some(field_value) = record_json.get(column_name) {
                    let json_key = serde_json::to_string(field_value).map_err(|e| {
                        RedbError::Query(format!("JSON key serialization failed: {}", e))
                    })?;
                    new_index_table
                        .insert(json_key.as_str(), record_id.value())
                        .map_err(|e| RedbError::Storage(Box::new(e)))?;
                }
            }
        }

        // Commit transaction
        write_txn
            .commit()
            .map_err(|e| RedbError::Commit(Box::new(e)))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl vantage_table::TableSource for Redb {
    type Column = crate::RedbColumn;
    type Expr = crate::expression::RedbExpression;

    fn create_column(&self, name: &str, table: impl vantage_table::TableLike) -> Self::Column {
        crate::RedbColumn::new(name, table.table_name().to_string())
    }

    fn expr(
        &self,
        _template: impl Into<String>,
        _parameters: Vec<vantage_expressions::protocol::expressive::IntoExpressive<Self::Expr>>,
    ) -> Self::Expr {
        panic!("ReDB is a key-value store and doesn't support SQL-like expressions")
    }

    async fn get_table_data<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<E>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        use vantage_core::util::error::vantage_error;
        use vantage_expressions::protocol::selectable::Selectable;

        // Use RedbSelect and execute_select approach
        let mut select = crate::RedbSelect::<E>::new();
        select.set_source(table.table_name(), None);

        // Apply table conditions
        for condition in table.conditions() {
            select.add_where_condition(condition.clone());
        }

        let records_result = self.redb_execute_select(&select).await;

        // Handle the Result<Vec<E>> - convert redb error to VantageError at boundary
        let records = records_result.map_err(|e| vantage_error!("ReDB error: {}", e))?;
        Ok(records)
    }

    async fn get_table_data_some<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Option<E>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        use vantage_core::util::error::vantage_error;
        use vantage_expressions::protocol::selectable::Selectable;

        // Use RedbSelect with limit 1
        let mut select = crate::RedbSelect::<E>::new();
        select.set_source(table.table_name(), None);

        // Apply table conditions
        for condition in table.conditions() {
            select.add_where_condition(condition.clone());
        }

        // Limit to 1 record for efficiency
        select.set_limit(Some(1), None);

        let records_result = self.redb_execute_select(&select).await;

        // Handle the Result<Vec<E>> - convert redb error to VantageError at boundary
        let records = records_result.map_err(|e| vantage_error!("ReDB error: {}", e))?;
        Ok(records.into_iter().next())
    }

    async fn get_table_data_as_value<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        use vantage_core::util::error::VantageError;

        // ReDB can't retrieve data as generic JSON values because:
        // 1. Data is stored as binary (bincode)
        // 2. We need the concrete type T to deserialize
        // 3. We can't convert arbitrary binary data to JSON
        Err(VantageError::no_capability(
            "get_table_data_values",
            "ReDB requires specific entity types for data retrieval - use Table<Redb, YourEntity>.get() instead",
        ))
    }
    async fn insert_table_data<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        record: E,
    ) -> vantage_dataset::dataset::Result<Option<String>>
    where
        E: vantage_core::Entity + serde::Serialize,
        Self: Sized,
    {
        use uuid::Uuid;
        use vantage_core::util::error::{Context, vantage_error};

        let table_name = table.table_name();
        let record_id = Uuid::new_v4().to_string();

        // Serialize the record
        let serialized = bincode::serialize(&record).context("Serialization failed")?;

        // Track failed column indexes
        let mut failed_columns = Vec::new();

        // Begin write transaction
        let write_txn = self
            .begin_write()
            .map_err(|e| vantage_error!("Failed to begin write transaction: {}", e))?;

        {
            // Insert into main table
            let main_table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
            let mut main_table = write_txn
                .open_table(main_table_def)
                .map_err(|e| vantage_error!("Failed to open main table: {}", e))?;

            main_table
                .insert(record_id.as_str(), serialized.as_slice())
                .map_err(|e| vantage_error!("Failed to insert into main table: {}", e))?;

            // Update index tables for each column
            let record_json =
                serde_json::to_value(&record).context("Failed to serialize record to JSON")?;

            for (_column_name, column) in table.columns() {
                let column_name = column.name();
                if let Some(field_value) = record_json.get(column_name) {
                    let json_key = serde_json::to_string(field_value)
                        .context("JSON key serialization failed")?;

                    let index_table_name = format!("{}_by_{}", table_name, column_name);
                    let index_table_def: TableDefinition<&str, &str> =
                        TableDefinition::new(&index_table_name);

                    // Try to open and update index table
                    match write_txn.open_table(index_table_def) {
                        Ok(mut index_table) => {
                            if index_table
                                .insert(json_key.as_str(), record_id.as_str())
                                .is_err()
                            {
                                // Record this column as failed
                                failed_columns.push(column_name.to_string());
                            }
                        }
                        Err(_) => {
                            // Index table doesn't exist, record as failed for rebuild
                            failed_columns.push(column_name.to_string());
                        }
                    }
                }
            }
        }

        // Commit transaction first
        write_txn
            .commit()
            .map_err(|e| vantage_error!("Failed to commit transaction: {}", e))?;

        // Rebuild any failed column indexes
        for column_name in failed_columns {
            self.redb_rebuild_index(table, &column_name)
                .await
                .map_err(|e| {
                    vantage_error!("Failed to rebuild index for column {}: {}", column_name, e)
                })?;
        }

        Ok(Some(record_id))
    }
}
