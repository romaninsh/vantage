use async_trait::async_trait;

use vantage_dataset::dataset::{Id, ReadableDataSet, Result};

use crate::{Entity, Table, TableSource};

// Single implementation for all TableSource types
#[async_trait]
impl<T, E> ReadableDataSet<E> for Table<T, E>
where
    T: TableSource + Clone,
    E: Entity,
{
    async fn get(&self) -> Result<Vec<E>> {
        self.data_source().get_table_data(self).await
    }

    async fn get_some(&self) -> Result<Option<E>> {
        self.data_source().get_table_data_some(self).await
    }

    /// get_id must be implemented properly for a specific table driver
    async fn get_id(&self, id: impl Id) -> Result<E> {
        self.data_source().get_table_data_by_id(self, id).await
    }

    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: Entity,
    {
        let t = self.clone().into_entity::<U>();
        self.data_source().get_table_data(&t).await
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: Entity,
    {
        let t = self.clone().into_entity::<U>();
        self.data_source().get_table_data_some(&t).await
    }

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        self.data_source().get_table_data_as_value(self).await
    }
}
