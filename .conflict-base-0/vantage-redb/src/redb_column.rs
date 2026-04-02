//! # Redb Column
//!
//! A redb-specific column implementation for key-value operations.
//! Since redb is a KV store, columns represent secondary index tables.

use crate::util::{Context, Result, vantage_error};
use redb::{TableDefinition, WriteTransaction};
use std::collections::{HashMap, HashSet};
use vantage_dataset::dataset::{ReadableDataSet, ReadableValueSet};
use vantage_expressions::{Expression, expr};
use vantage_table::{ColumnFlag, ColumnLike};

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

    /// Set the table name for this column
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = table.into();
        self
    }

    /// Get the index table name for this column
    pub fn index_table_name(&self) -> String {
        format!("{}_idx_{}", self.table, self.name)
    }

    pub async fn rebuild_index(
        &self,
        data: &(impl ReadableDataSet<serde_json::Value> + ReadableValueSet),
        write_txn: &WriteTransaction,
    ) -> Result<()> {
        let values = data.list_values().await?;

        // Group records by column value - each value maps to a list of IDs
        let mut index: HashMap<String, Vec<String>> = HashMap::new();

        for record in values {
            let id = record["id"]
                .as_str()
                .ok_or_else(|| vantage_error!("Record missing id field"))?
                .to_string();

            let value = match &record[&self.name] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => "null".to_string(),
                _ => {
                    return Err(
                        vantage_error!("Unsupported value type for field {}", self.name).into(),
                    );
                }
            };

            index.entry(value).or_default().push(id);
        }

        // Create index table and populate it
        let table_name = self.index_table_name();
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(&table_name);
        let mut index_table = write_txn
            .open_table(table_def)
            .context("Failed to open index table")?;

        // Clear existing index entries
        index_table
            .retain(|_, _| false)
            .context("Failed to clear index table")?;

        for (value, ids) in index {
            let serialized_ids = bincode::serialize(&ids).context("Failed to serialize IDs")?;

            index_table
                .insert(value.as_str(), serialized_ids.as_slice())
                .context("Failed to insert index entry")?;
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

    fn flags(&self) -> HashSet<ColumnFlag> {
        HashSet::new()
    }

    fn get_type(&self) -> &'static str {
        "any"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl From<&str> for RedbColumn {
    fn from(name: &str) -> Self {
        // Use empty table name - will be set properly when added to a table
        Self::new(name, String::new())
    }
}

impl From<String> for RedbColumn {
    fn from(name: String) -> Self {
        // Use empty table name - will be set properly when added to a table
        Self::new(name, String::new())
    }
}

/// Operations available on redb columns
pub trait RedbColumnOperations {
    /// Create equality condition for this column
    fn eq<T: Into<serde_json::Value>>(&self, value: T) -> crate::expression::RedbExpression;
}

impl RedbColumnOperations for RedbColumn {
    fn eq<T: Into<serde_json::Value>>(&self, value: T) -> crate::expression::RedbExpression {
        use crate::expression::RedbExpression;
        RedbExpression::eq(self.name.clone(), value.into())
    }
}
