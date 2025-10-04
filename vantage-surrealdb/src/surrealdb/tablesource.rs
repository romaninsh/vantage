use crate::SurrealDB;
use async_trait::async_trait;
use vantage_dataset::dataset::DataSetError;

#[async_trait]
impl vantage_table::TableSource for SurrealDB {
    type Column = crate::SurrealColumn;

    fn create_column(&self, name: &str, _table: impl vantage_table::TableLike) -> Self::Column {
        crate::SurrealColumn::new(name)
    }

    async fn get_table_data_as<T>(
        &self,
        _table_name: &str,
    ) -> vantage_dataset::dataset::Result<Vec<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        Err(DataSetError::no_capability(
            "get_table_data_as",
            "SurrealDB",
        ))
    }

    async fn get_table_data_some_as<T>(
        &self,
        _table_name: &str,
    ) -> vantage_dataset::dataset::Result<Option<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        Err(DataSetError::no_capability(
            "get_table_data_some_as",
            "SurrealDB",
        ))
    }

    async fn get_table_data_values(
        &self,
        _table_name: &str,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>> {
        Err(DataSetError::no_capability(
            "get_table_data_values",
            "SurrealDB",
        ))
    }
}
