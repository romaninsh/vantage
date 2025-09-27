use async_trait::async_trait;
use serde::de::DeserializeOwned;
use vantage_dataset::dataset::{DataSetError, Id, ReadableDataSet, Result};
use vantage_expressions::Expression;

use crate::{Entity, QuerySource, Table, TableSource};

#[async_trait]
impl<T, E, R> ReadableDataSet<R> for Table<T, E>
where
    T: QuerySource<Expression> + TableSource + Send + Sync,
    E: Entity + Send + Sync,
    R: DeserializeOwned + Send + Sync,
{
    async fn get(&self) -> Result<Vec<R>> {
        <Self as ReadableDataSet<R>>::get_as::<R>(self).await
    }

    async fn get_some(&self) -> Result<Option<R>> {
        <Self as ReadableDataSet<R>>::get_some_as::<R>(self).await
    }

    /// get_id must be implemented properly for a specific table driver
    async fn get_id(&self, _id: impl Id) -> Result<R> {
        let _id = _id.into();
        Err(DataSetError::no_capability("get_id", "Table"))
    }

    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: DeserializeOwned,
    {
        Err(DataSetError::no_capability("get_as", "Table"))
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: DeserializeOwned,
    {
        Err(DataSetError::no_capability("get_some_as", "Table"))
    }
}
