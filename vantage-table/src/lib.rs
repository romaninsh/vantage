//! # Vantage Table
//!
//! A clean table abstraction for the Vantage framework, defined over a datasource and entity.
//!
//! ## Example
//!
//! ```rust,ignore
//! use vantage_table::{Table, Column, EmptyEntity};
//! use vantage_expressions::expr;
//!
//! // Create a new table with a datasource
//! let mut table = Table::new("users", my_datasource);
//!
//! // Add columns
//! table.add_column(Column::new("name"));
//! table.add_column(Column::new("email").with_alias("user_email"));
//!
//! // Add conditions
//! table.add_condition(expr!("age > {}", 18));
//! table.add_condition(expr!("status = {}", "active"));
//!
//! // Or use the builder pattern
//! let table = Table::new("users", my_datasource)
//!     .with(|t| {
//!         t.add_column(Column::new("name"));
//!         t.add_condition(expr!("active = {}", true));
//!     });
//! ```

use indexmap::IndexMap;
use std::marker::PhantomData;
use vantage_expressions::{OwnedExpression, protocol::selectable::Selectable};

pub mod readable;

/// Re-export DataSource from vantage-expressions for convenience
pub use vantage_expressions::protocol::datasource::DataSource;

/// Trait for entities that can be associated with tables
pub trait Entity {
    // Placeholder for entity trait
}

/// Empty entity type for tables without a specific entity
pub struct EmptyEntity;
impl Entity for EmptyEntity {}

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

/// A table abstraction defined over a datasource and entity
#[derive(Debug)]
pub struct Table<T, E>
where
    T: DataSource<OwnedExpression>,
    E: Entity,
{
    data_source: T,
    _phantom: PhantomData<E>,
    table_name: String,
    columns: IndexMap<String, Column>,
    conditions: Vec<OwnedExpression>,
}

impl<T: DataSource<OwnedExpression>> Table<T, EmptyEntity> {
    /// Create a new table with the given name and datasource
    pub fn new(table_name: impl Into<String>, data_source: T) -> Self {
        Self {
            data_source,
            _phantom: PhantomData,
            table_name: table_name.into(),
            columns: IndexMap::new(),
            conditions: Vec::new(),
        }
    }
}

impl<T: DataSource<OwnedExpression>, E: Entity> Table<T, E> {
    /// Use a callback with a builder pattern for configuration
    pub fn with<F>(mut self, func: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        func(&mut self);
        self
    }

    /// Convert this table to use a different entity type
    pub fn into_entity<E2: Entity>(self) -> Table<T, E2> {
        Table {
            data_source: self.data_source,
            _phantom: PhantomData,
            table_name: self.table_name,
            columns: self.columns,
            conditions: self.conditions,
        }
    }

    /// Add a column to the table
    pub fn add_column(&mut self, column: Column) {
        self.columns.insert(column.name().to_string(), column);
    }

    /// Add a condition to limit what records the table represents
    pub fn add_condition(&mut self, condition: OwnedExpression) {
        self.conditions.push(condition);
    }

    /// Get the table name
    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    /// Get all columns
    pub fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }

    /// Get all conditions
    pub fn conditions(&self) -> &[OwnedExpression] {
        &self.conditions
    }

    /// Get the underlying data source
    pub fn data_source(&self) -> &T {
        &self.data_source
    }

    /// Create a select query with table configuration applied
    pub fn select(&self) -> impl Selectable {
        let mut select = self.data_source.select();

        // Set the table as source
        select.set_source(self.table_name.as_str(), None);

        // Add all columns as fields
        for column in self.columns.values() {
            match column.alias() {
                Some(alias) => select.add_expression(
                    vantage_expressions::expr!(column.name()),
                    Some(alias.to_string()),
                ),
                None => select.add_field(column.name()),
            }
        }

        // Add all conditions
        for condition in &self.conditions {
            select.add_where_condition(condition.clone());
        }

        select
    }

    /// Get data from the table using the configured columns and conditions
    pub async fn get(&self) -> serde_json::Value {
        let select = self.select();
        self.data_source.execute(&select.into()).await
    }
}
