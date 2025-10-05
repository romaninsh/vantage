use crate::{Entity, TableLike};
use async_trait::async_trait;
use vantage_expressions::{Expression, protocol::datasource::DataSource};

/// Trait for table data sources that defines column type separate from execution
/// TableSource represents a data source that can create and manage tables
#[async_trait]
pub trait TableSource: DataSource {
    type Column: ColumnLike + Clone + 'static;
    type Expr: Clone + Send + Sync + 'static;

    /// Create a new column with the given name
    fn create_column(&self, name: &str, table: impl TableLike) -> Self::Column;

    /// Create an expression from a template and parameters, similar to Expression::new
    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<vantage_expressions::protocol::expressive::IntoExpressive<Self::Expr>>,
    ) -> Self::Expr;

    /// Get all data from a table as the table's entity type
    async fn get_table_data<E>(
        &self,
        table: &crate::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<E>>
    where
        E: Entity,
        Self: Sized;

    /// Get some data from a table as the table's entity type (usually first record)
    async fn get_table_data_some<E>(
        &self,
        table: &crate::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Option<E>>
    where
        E: Entity,
        Self: Sized;

    /// Get raw JSON values from a table without deserializing to a specific type
    async fn get_table_data_as_value<E>(
        &self,
        table: &crate::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>>
    where
        E: Entity,
        Self: Sized;

    /// Insert a record into the table and return generated ID
    async fn insert_table_data<E>(
        &self,
        table: &crate::Table<Self, E>,
        record: E,
    ) -> vantage_dataset::dataset::Result<Option<String>>
    where
        E: Entity + serde::Serialize,
        Self: Sized;
}

/// Minimal trait for column-like objects
pub trait ColumnLike: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn alias(&self) -> Option<&str>;
    fn expr(&self) -> Expression;
}
