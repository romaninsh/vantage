use async_trait::async_trait;
use serde::de::DeserializeOwned;
use vantage_dataset::dataset::{Id, ReadableDataSet, Result};
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

    async fn get_id(&self, _id: impl Id) -> Result<R> {
        let _id = _id.into();
        todo!("Implement get_id for Table")
    }

    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: DeserializeOwned,
    {
        todo!("Implement get_as for Table")
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: DeserializeOwned,
    {
        todo!("Implement get_some_as for Table")
    }
}
