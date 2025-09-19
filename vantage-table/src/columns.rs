use indexmap::IndexMap;
use vantage_expressions::{DataSource, OwnedExpression};

use super::{Entity, Table};

/// Represents a table column with optional alias
#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    alias: Option<String>,
}

impl Column {
    /// Create a new column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
        }
    }

    /// Set an alias for this column
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Get the column name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the column alias if set
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

impl<T: DataSource<OwnedExpression>, E: Entity> Table<T, E> {
    /// Add a column to the table
    pub fn add_column(&mut self, column: Column) {
        self.columns.insert(column.name().to_string(), column);
    }

    /// Get all columns
    pub fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }
}
