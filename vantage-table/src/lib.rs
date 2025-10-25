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

use async_trait::async_trait;
use indexmap::IndexMap;
use std::marker::PhantomData;
use std::sync::Arc;
use vantage_expressions::SelectSource;

use vantage_core::{
    Result, error,
    util::error::{Context, vantage_error},
};
use vantage_expressions::{Expression, protocol::selectable::Selectable};

pub mod insertable;
pub mod mocks;
pub mod models_macro;
pub mod prelude;
pub mod readable;
pub mod record;
pub mod tablesource;
pub mod with_columns;
pub mod with_conditions;
pub mod writable;

/// Re-export ColumnLike from vantage-expressions for convenience
pub use crate::tablesource::ColumnLike;
/// Re-export DataSource from vantage-expressions for convenience
pub use vantage_expressions::QuerySource;

pub use crate::tablesource::TableSource;
pub use crate::with_columns::{Column, ColumnFlag};

/// Trait for dynamic table operations without generics
#[async_trait]
pub trait TableLike: Send + Sync {
    /// Get all columns as boxed ColumnLike trait objects
    fn columns(&self) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>>;
    fn get_column(&self, name: &str) -> Option<Arc<dyn ColumnLike>>;

    fn table_name(&self) -> &str;
    fn table_alias(&self) -> &str;

    /// Get raw data from the table as `Vec<Value>` for UI grids
    async fn get_values(&self) -> Result<Vec<serde_json::Value>>;
}

// Re-export Entity trait from vantage-core
pub use vantage_core::Entity;

/// Empty entity type for tables without a specific entity
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
pub struct EmptyEntity;

/// Entity that contains ID only
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
pub struct IdEntity {
    pub id: String,
}

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
    conditions: Vec<T::Expr>,
}

impl<T: TableSource> Table<T, EmptyEntity>
where
    T::Column: ColumnLike,
{
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
    T: TableSource + SelectSource<T::Expr>,
    E: Entity,
{
    /// Get data from the table using the configured columns and conditions
    pub async fn get(&self) -> Result<Vec<E>> {
        // Use TableSource directly instead of QuerySource
        let entities_with_ids = self
            .data_source
            .get_table_data(self)
            .await
            .with_context(|| error!("Failed to get table data"))?;
        Ok(entities_with_ids
            .into_iter()
            .map(|(_, entity)| entity)
            .collect())
    }

    /// Get raw data from the table as `Vec<Value>` without entity deserialization
    pub async fn get_values(&self) -> Result<Vec<serde_json::Value>>
    where
        T: QuerySource<T::Expr>,
        T::Select<E>: Into<T::Expr>,
    {
        let select = self.select();
        let raw_result = self.data_source.execute(&select.into()).await;

        // Try to parse as array of objects
        if let serde_json::Value::Array(items) = raw_result {
            Ok(items)
        } else {
            Err(vantage_error!("Expected array of objects from database"))
        }
    }

    /// Create a select query with table configuration applied
    pub fn select(&self) -> T::Select<E> {
        let mut select = self.data_source.select::<E>();

        // Set the table as source
        select.set_source(self.table_name.as_str(), None);

        // Add all columns as fields
        for column in self.columns.values() {
            match column.alias() {
                Some(alias) => select.add_expression(
                    self.data_source.expr(column.name(), vec![]),
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

#[async_trait]
impl<T: TableSource, E: Entity> TableLike for Table<T, E>
where
    T: TableSource + Send + Sync,
    T::Column: ColumnLike + Clone + 'static,
    E: Send + Sync,
{
    fn columns(&self) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>> {
        let arc_columns: IndexMap<String, Arc<dyn ColumnLike>> = self
            .columns
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone()) as Arc<dyn ColumnLike>))
            .collect();
        Arc::new(arc_columns)
    }

    fn get_column(&self, name: &str) -> Option<Arc<dyn ColumnLike>> {
        self.columns
            .get(name)
            .map(|col| Arc::new(col.clone()) as Arc<dyn ColumnLike>)
    }

    fn table_alias(&self) -> &str {
        &self.table_name
    }

    fn table_name(&self) -> &str {
        &self.table_name
    }

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        self.data_source
            .get_table_data_as_value(self)
            .await
            .map_err(|e| vantage_error!("Failed to get table values: {}", e))
    }
}

#[async_trait]
impl<T: TableSource, E: Entity> TableLike for &Table<T, E>
where
    T: TableSource + Send + Sync,
    T::Column: ColumnLike + Clone + 'static,
    E: Send + Sync,
{
    fn columns(&self) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>> {
        let arc_columns: IndexMap<String, Arc<dyn ColumnLike>> = self
            .columns
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone()) as Arc<dyn ColumnLike>))
            .collect();
        Arc::new(arc_columns)
    }

    fn get_column(&self, name: &str) -> Option<Arc<dyn ColumnLike>> {
        self.columns
            .get(name)
            .map(|col| Arc::new(col.clone()) as Arc<dyn ColumnLike>)
    }

    fn table_alias(&self) -> &str {
        &self.table_name
    }

    fn table_name(&self) -> &str {
        &self.table_name
    }

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        self.data_source
            .get_table_data_as_value(self)
            .await
            .map_err(|e| vantage_error!("Failed to get table values: {}", e))
    }
}
