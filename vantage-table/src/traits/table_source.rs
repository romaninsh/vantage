use std::hash::Hash;

use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    Expression, traits::datasource::DataSource, traits::expressive::ExpressiveEnum,
};
use vantage_types::{Entity, Record};

use crate::{
    column::column::ColumnType,
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike},
};

/// Trait for table data sources that defines column type separate from execution
/// TableSource represents a data source that can create and manage tables
#[async_trait]
pub trait TableSource: DataSource + Clone + 'static {
    type Column<Type>: ColumnLike<Type> + Clone
    where
        Type: ColumnType;
    type AnyType: ColumnType;
    type Value: Clone + Send + Sync + 'static;
    type Id: Send + Sync + Clone + Hash + Eq + 'static;

    /// Create a new column with the given name
    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type>;

    /// Convert a typed column to type-erased column
    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType>;

    /// Attempt to convert a type-erased column back to typed column
    fn from_any_column<Type: ColumnType>(
        &self,
        any_column: &Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>>;

    /// Create an expression from a template and parameters, similar to Expression::new
    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value>;

    /// Create a search expression for a table (e.g., searches across searchable fields)
    ///
    /// Different vendors implement search differently:
    /// - SQL: `field LIKE '%value%'`
    /// - SurrealDB: `field CONTAINS 'value'` or `field ~ 'value'`
    /// - MongoDB: `{ field: { $regex: 'value', $options: 'i' } }`
    ///
    /// The implementation should search across appropriate fields in the table.
    fn search_expression(
        &self,
        table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value>;

    /// Get all data from a table as Record values with IDs (for ReadableValueSet implementation)
    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get a single record by ID as Record value (for ReadableValueSet implementation)
    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get some data from a table as Record value with ID (for ReadableValueSet implementation)
    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get count of records in the table
    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get sum of a column in the table
    async fn get_sum<E, Type: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Insert a record as Record value (for WritableValueSet implementation)
    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Replace a record as Record value (for WritableValueSet implementation)
    async fn replace_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Patch a record as Record value (for WritableValueSet implementation)
    async fn patch_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Delete a record by ID (for WritableValueSet implementation)
    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Delete all records (for WritableValueSet implementation)
    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Insert a record and return generated ID (for InsertableValueSet implementation)
    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
        Self: Sized;
}
