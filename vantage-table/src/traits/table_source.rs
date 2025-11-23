use async_trait::async_trait;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    Expression, traits::datasource::DataSource, traits::expressive::ExpressiveEnum,
};
use vantage_types::Entity;

use crate::{
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike},
};

/// Trait for table data sources that defines column type separate from execution
/// TableSource represents a data source that can create and manage tables
#[async_trait]
pub trait TableSource: DataSource + Clone + 'static {
    type Column: ColumnLike + Clone + 'static;
    type Value: Clone + Send + Sync + 'static;
    type Id: 'static;

    /// Create a new column with the given name
    fn create_column(&self, name: &str, table: impl TableLike) -> Self::Column;

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

    /// Get all data from a table as the table's entity type with IDs
    async fn get_table_data<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::traits::Result<Vec<(String, E)>>
    where
        E: Entity,
        Self: Sized;

    /// Get some data from a table as the table's entity type (usually first record) with ID
    async fn get_table_data_some<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::traits::Result<Option<(String, E)>>
    where
        E: Entity,
        Self: Sized;

    /// Get raw JSON values from a table without deserializing to a specific type
    async fn get_table_data_as_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::traits::Result<Vec<serde_json::Value>>
    where
        E: Entity,
        Self: Sized;

    /// Get a single record by ID as raw JSON value
    async fn get_table_data_as_value_by_id<E>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<serde_json::Value>
    where
        E: Entity,
        Self: Sized;

    /// Get some data from a table as raw JSON value (usually first record)
    async fn get_table_data_as_value_some<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<serde_json::Value>>
    where
        E: Entity,
        Self: Sized;

    /// Insert a record into the table and return generated ID
    async fn insert_table_data<E>(
        &self,
        table: &Table<Self, E>,
        record: E,
    ) -> vantage_dataset::traits::Result<Option<String>>
    where
        E: Entity + serde::Serialize,
        Self: Sized;

    /// Insert a record with a specific ID, fails if ID already exists
    async fn insert_table_data_with_id<E>(
        &self,
        table: &Table<Self, E>,
        id: Self::Id,
        record: E,
    ) -> Result<()>
    where
        E: Entity + serde::Serialize,
        Self: Sized;

    /// Replace a record by ID (upsert - creates if missing, replaces if exists)
    async fn replace_table_data_with_id<E>(
        &self,
        table: &Table<Self, E>,
        id: Self::Id,
        record: E,
    ) -> Result<()>
    where
        E: Entity + serde::Serialize,
        Self: Sized;

    /// Partially update a record by ID, fails if record doesn't exist
    async fn patch_table_data_with_id<E>(
        &self,
        table: &Table<Self, E>,
        id: Self::Id,
        partial: serde_json::Value,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized;

    /// Delete a record by ID
    async fn delete_table_data_with_id<E>(
        &self,
        table: &Table<Self, E>,
        id: Self::Id,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized;

    /// Update records using a callback that modifies each record in place
    async fn update_table_data<E, F>(&self, table: &Table<Self, E>, callback: F) -> Result<()>
    where
        E: Entity,
        F: Fn(&mut E) + Send + Sync,
        Self: Sized;

    /// Delete all records in the table
    async fn delete_table_data<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity,
        Self: Sized;

    /// Get a record by ID
    async fn get_table_data_by_id<E>(&self, table: &Table<Self, E>, id: Self::Id) -> Result<E>
    where
        E: Entity,
        Self: Sized;

    /// Get count of records in the table
    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity,
        Self: Sized;

    /// Get sum of a column in the table
    async fn get_sum<E>(&self, table: &Table<Self, E>, column: &Self::Column) -> Result<i64>
    where
        E: Entity,
        Self: Sized;

    /// Insert a record with a specific ID using JSON value, fails if ID already exists
    async fn insert_table_data_with_id_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: serde_json::Value,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized;

    /// Replace a record by ID using JSON value (upsert - creates if missing, replaces if exists)
    async fn replace_table_data_with_id_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: serde_json::Value,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized;

    /// Update records using a callback that modifies each record in place as JSON values
    async fn update_table_data_value<E, F>(
        &self,
        table: &Table<Self, E>,
        callback: F,
    ) -> Result<()>
    where
        E: Entity,
        F: Fn(&mut serde_json::Value) + Send + Sync,
        Self: Sized;
}
