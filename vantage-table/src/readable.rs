use async_trait::async_trait;

use vantage_dataset::dataset::{Id, ReadableAsDataSet, ReadableDataSet, ReadableValueSet, Result};

use crate::{Entity, Table, TableSource};

// Implementation for ReadableDataSet<E>
#[async_trait]
impl<T, E> ReadableDataSet<E> for Table<T, E>
where
    T: TableSource,
    E: Entity,
{
    async fn get(&self) -> Result<Vec<E>> {
        let results = self.data_source().get_table_data(self).await?;
        Ok(results.into_iter().map(|(_, entity)| entity).collect())
    }

    async fn get_some(&self) -> Result<Option<E>> {
        let result = self.data_source().get_table_data_some(self).await?;
        Ok(result.map(|(_, entity)| entity))
    }

    async fn get_id(&self, id: impl Id) -> Result<E> {
        self.data_source().get_table_data_by_id(self, id).await
    }
}

// Implementation for ReadableValueSet
#[async_trait]
impl<T, E> ReadableValueSet for Table<T, E>
where
    T: TableSource,
    E: Entity,
{
    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        self.data_source().get_table_data_as_value(self).await
    }

    async fn get_id_value(&self, id: &str) -> Result<serde_json::Value> {
        self.data_source()
            .get_table_data_as_value_by_id(self, id)
            .await
    }

    async fn get_some_value(&self) -> Result<Option<serde_json::Value>> {
        self.data_source().get_table_data_as_value_some(self).await
    }
}

// Implementation for ReadableAsDataSet
#[async_trait]
impl<T, E> ReadableAsDataSet for Table<T, E>
where
    T: TableSource,
    E: Entity,
{
    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: Entity,
    {
        let values = self.data_source().get_table_data_as_value(self).await?;
        let mut results = Vec::new();
        for value in values {
            match serde_json::from_value::<U>(value) {
                Ok(entity) => results.push(entity),
                Err(e) => {
                    return Err(vantage_core::util::error::vantage_error!(
                        "Failed to deserialize to target type: {}",
                        e
                    ));
                }
            }
        }
        Ok(results)
    }

    async fn get_id_as<U>(&self, id: &str) -> Result<U>
    where
        U: Entity,
    {
        let value = self
            .data_source()
            .get_table_data_as_value_by_id(self, id)
            .await?;
        match serde_json::from_value::<U>(value) {
            Ok(entity) => Ok(entity),
            Err(e) => Err(vantage_core::util::error::vantage_error!(
                "Failed to deserialize to target type: {}",
                e
            )),
        }
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: Entity,
    {
        let values = self.data_source().get_table_data_as_value(self).await?;
        if let Some(first_value) = values.into_iter().next() {
            match serde_json::from_value::<U>(first_value) {
                Ok(entity) => Ok(Some(entity)),
                Err(e) => Err(vantage_core::util::error::vantage_error!(
                    "Failed to deserialize to target type: {}",
                    e
                )),
            }
        } else {
            Ok(None)
        }
    }
}
