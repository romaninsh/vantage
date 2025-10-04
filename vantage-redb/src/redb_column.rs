//! # Redb Column
//!
//! A redb-specific column implementation for key-value operations.
//! Since redb is a KV store, columns represent secondary index tables.

use redb::{TableDefinition, WriteTransaction};
use serde_json::Value;
use std::collections::HashMap;
use vantage_dataset::dataset::{DataSetError, ReadableDataSet, Result};
use vantage_expressions::{Expression, expr};
use vantage_table::ColumnLike;

/// Redb-specific column that represents a secondary index table
#[derive(Debug, Clone)]
pub struct RedbColumn {
    table: String,
    name: String,
    alias: Option<String>,
}

impl RedbColumn {
    /// Create a new redb column with the given name
    pub fn new(name: impl Into<String>, table: String) -> Self {
        Self {
            table,
            name: name.into(),
            alias: None,
        }
    }

    /// Set an alias for this column
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Get the index table name for this column
    pub fn index_table_name(&self) -> String {
        format!("{}_idx_{}", self.table, self.name)
    }

    pub async fn rebuild_index(
        &self,
        data: &impl ReadableDataSet<serde_json::Value>,
        write_txn: &WriteTransaction,
    ) -> Result<()> {
        let values = data.get_values().await?;

        // Group records by column value - each value maps to a list of IDs
        let mut index: HashMap<String, Vec<String>> = HashMap::new();

        for record in values {
            let id = record["id"]
                .as_str()
                .ok_or_else(|| DataSetError::other("Record missing id field"))?
                .to_string();

            let value = match &record[&self.name] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => "null".to_string(),
                _ => {
                    return Err(DataSetError::other(format!(
                        "Unsupported value type for field {}",
                        self.name
                    )));
                }
            };

            index.entry(value).or_insert_with(Vec::new).push(id);
        }

        // Create index table and populate it
        let table_name = self.index_table_name();
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(&table_name);
        let mut index_table = write_txn
            .open_table(table_def)
            .map_err(|e| DataSetError::other(format!("Failed to open index table: {}", e)))?;

        // Clear existing index entries
        index_table
            .retain(|_, _| false)
            .map_err(|e| DataSetError::other(format!("Failed to clear index table: {}", e)))?;

        for (value, ids) in index {
            let serialized_ids = bincode::serialize(&ids)
                .map_err(|e| DataSetError::other(format!("Failed to serialize IDs: {}", e)))?;

            index_table
                .insert(value.as_str(), serialized_ids.as_slice())
                .map_err(|e| DataSetError::other(format!("Failed to insert index entry: {}", e)))?;
        }

        Ok(())
    }
}

impl ColumnLike for RedbColumn {
    fn name(&self) -> &str {
        &self.name
    }

    fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    fn expr(&self) -> Expression {
        // For redb, column expressions are just field references
        expr!("{}", self.name.clone())
    }
}

/// Operations available on redb columns
pub trait RedbColumnOperations {
    /// Create equality condition for this column
    fn eq<T: Into<serde_json::Value>>(self, value: T) -> Expression;
}

impl RedbColumnOperations for RedbColumn {
    fn eq<T: Into<serde_json::Value>>(self, _value: T) -> Expression {
        todo!("Implement equality condition for redb column")
    }
}
