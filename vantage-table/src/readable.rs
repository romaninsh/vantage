use async_trait::async_trait;

use vantage_dataset::dataset::{Id, ReadableDataSet, Result};

use crate::{Entity, Table, TableSource};

// Single implementation for all TableSource types
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

    /// get_id must be implemented properly for a specific table driver
    async fn get_id(&self, id: impl Id) -> Result<E> {
        self.data_source().get_table_data_by_id(self, id).await
    }

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

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        self.data_source().get_table_data_as_value(self).await
    }
}
