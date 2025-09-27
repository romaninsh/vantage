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
use vantage_expressions::SelectSource;
use vantage_expressions::mocks::StaticDataSource;
use vantage_expressions::{Expression, protocol::selectable::Selectable, util::error::Result};

pub mod mocks;
pub mod prelude;
pub mod readable;
pub mod tablesource;
pub mod with_columns;
pub mod with_conditions;

/// Re-export ColumnLike from vantage-expressions for convenience
pub use crate::tablesource::ColumnLike;
/// Re-export DataSource from vantage-expressions for convenience
pub use vantage_expressions::QuerySource;

pub use crate::tablesource::TableSource;
pub use crate::with_columns::Column;

// Specific implementation for StaticDataSource
impl TableSource for StaticDataSource {
    type Column = mocks::MockColumn;

    fn create_column(&self, name: &str, _table: impl TableLike) -> Self::Column {
        name.into()
    }
}

/// Trait for dynamic table operations without generics
pub trait TableLike: Send + Sync {
    /// Get all columns as boxed ColumnLike trait objects
    fn columns(&self) -> Vec<Box<dyn ColumnLike>>;

    fn table_alias(&self) -> &str;
}

/// Trait for entities that can be associated with tables
pub trait Entity:
    serde::Serialize + serde::de::DeserializeOwned + Default + Clone + Send + Sync + Sized + 'static
{
}

/// Empty entity type for tables without a specific entity
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct EmptyEntity;
impl Entity for EmptyEntity {}

/// A table abstraction defined over a datasource and entity
#[derive(Debug, Clone)]
pub struct Table<T, E>
where
    T: TableSource,
    E: Entity,
{
    data_source: T,
    _phantom: PhantomData<E>,
    table_name: String,
    columns: IndexMap<String, T::Column>,
    conditions: Vec<Expression>,
}

impl<T: TableSource> Table<T, EmptyEntity> {
    /// Create a new table with the given name and table source
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

impl<T: TableSource, E: Entity> Table<T, E> {
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
    /// Get the table name
    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    /// Get the underlying data source
    pub fn data_source(&self) -> &T {
        &self.data_source
    }
}

impl<T, E> Table<T, E>
where
    T: TableSource + SelectSource,
    E: Entity,
{
    /// Get data from the table using the configured columns and conditions
    pub async fn get(&self) -> Result<Vec<E>>
    where
        T: QuerySource<Expression>,
        T::Select: Into<Expression>,
    {
        let values = self.get_values().await?;
        let entities = values
            .into_iter()
            .map(|item| serde_json::from_value::<E>(item))
            .collect::<std::result::Result<Vec<E>, _>>()
            .map_err(|e| vantage_expressions::util::error::Error::new(e.to_string()))?;
        Ok(entities)
    }

    /// Get raw data from the table as `Vec<Value>` without entity deserialization
    pub async fn get_values(&self) -> Result<Vec<serde_json::Value>>
    where
        T: QuerySource<Expression>,
        T::Select: Into<Expression>,
    {
        let select = self.select();
        let raw_result = self.data_source.execute(&select.into()).await;

        // Try to parse as array of objects
        if let serde_json::Value::Array(items) = raw_result {
            Ok(items)
        } else {
            Err(vantage_expressions::util::error::Error::new(
                "Expected array of objects from database",
            ))
        }
    }

    /// Create a select query with table configuration applied
    pub fn select(&self) -> T::Select {
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
}

impl<T: TableSource, E: Entity> TableLike for Table<T, E> {
    fn columns(&self) -> Vec<Box<dyn ColumnLike>> {
        self.columns
            .values()
            .map(|col| Box::new(col.clone()) as Box<dyn ColumnLike>)
            .collect()
    }

    fn table_alias(&self) -> &str {
        &self.table_name
    }
}
