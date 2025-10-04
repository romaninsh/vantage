use crate::SurrealDB;
use async_trait::async_trait;
use vantage_dataset::dataset::DataSetError;
use vantage_expressions::Expression;
use vantage_table::Table;

#[async_trait]
impl vantage_table::TableSource for SurrealDB {
    type Column = crate::SurrealColumn;
    type Expr = Expression;

    fn create_column(&self, name: &str, _table: impl vantage_table::TableLike) -> Self::Column {
        crate::SurrealColumn::new(name)
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<vantage_expressions::protocol::expressive::IntoExpressive<Self::Expr>>,
    ) -> Self::Expr {
        Expression::new(template, parameters)
    }

    async fn get_table_data_as<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<E>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        Err(DataSetError::no_capability(
            "get_table_data_as",
            "SurrealDB",
        ))
    }

    async fn get_table_data_some_as<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Option<E>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        Err(DataSetError::no_capability(
            "get_table_data_some_as",
            "SurrealDB",
        ))
    }

    async fn get_table_data_values<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        Err(DataSetError::no_capability(
            "get_table_data_values",
            "SurrealDB",
        ))
    }
}
