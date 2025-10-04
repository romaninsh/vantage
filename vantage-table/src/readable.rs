use async_trait::async_trait;

use vantage_dataset::dataset::{DataSetError, Id, ReadableDataSet, Result};

use crate::{Entity, Table, TableSource};

// Single implementation for all TableSource types
#[async_trait]
impl<T, E> ReadableDataSet<E> for Table<T, E>
where
    T: TableSource,
    E: Entity,
{
    async fn get(&self) -> Result<Vec<E>> {
        self.data_source()
            .get_table_data_as(self.table_name())
            .await
    }

    async fn get_some(&self) -> Result<Option<E>> {
        self.data_source()
            .get_table_data_some_as(self.table_name())
            .await
    }

    /// get_id must be implemented properly for a specific table driver
    async fn get_id(&self, _id: impl Id) -> Result<E> {
        let _id = _id.into();
        Err(DataSetError::no_capability("get_id", "Table"))
    }

    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: Entity,
    {
        self.data_source()
            .get_table_data_as(self.table_name())
            .await
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: Entity,
    {
        self.data_source()
            .get_table_data_some_as(self.table_name())
            .await
    }

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        self.data_source()
            .get_table_data_values(self.table_name())
            .await
    }
}
