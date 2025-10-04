use crate::{Entity, TableLike};
use async_trait::async_trait;
use vantage_expressions::{Expression, protocol::datasource::DataSource};

/// Trait for table data sources that defines column type separate from execution
/// TableSource represents a data source that can create and manage tables
#[async_trait]
pub trait TableSource: DataSource {
    type Column: ColumnLike + Clone + 'static;

    /// Create a new column with the given name
    fn create_column(&self, name: &str, table: impl TableLike) -> Self::Column;

    /// Get all data from a table as a specific type
    async fn get_table_data_as<T>(
        &self,
        table_name: &str,
    ) -> vantage_dataset::dataset::Result<Vec<T>>
    where
        T: Entity;

    /// Get first record from a table as a specific type
    async fn get_table_data_some_as<T>(
        &self,
        table_name: &str,
    ) -> vantage_dataset::dataset::Result<Option<T>>
    where
        T: Entity;

    /// Get all data from a table as JSON values
    async fn get_table_data_values(
        &self,
        table_name: &str,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>>;
}

/// Minimal trait for column-like objects
pub trait ColumnLike: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn alias(&self) -> Option<&str>;
    fn expr(&self) -> Expression;
}
