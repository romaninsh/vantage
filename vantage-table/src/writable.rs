//! Writable implementation for Table
//!
//! This module provides the WritableDataSet trait implementation for Table<T, E>
//! where T: TableSource, allowing tables to perform write operations by delegating
//! to their underlying data source.

use crate::{Entity, Table, TableSource};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use vantage_dataset::dataset::{Id, Result, WritableDataSet};

#[async_trait]
impl<T, E> WritableDataSet<E> for Table<T, E>
where
    T: TableSource + Send + Sync,
    E: Entity + Serialize + DeserializeOwned + Send + Sync,
{
    async fn insert_id(&self, id: impl Id, record: E) -> Result<()> {
        self.data_source
            .insert_table_data_with_id(self, id, record)
            .await
    }

    async fn replace_id(&self, id: impl Id, record: E) -> Result<()> {
        self.data_source
            .replace_table_data_with_id(self, id, record)
            .await
    }

    async fn patch_id(&self, id: impl Id, partial: serde_json::Value) -> Result<()> {
        self.data_source
            .patch_table_data_with_id(self, id, partial)
            .await
    }

    async fn delete_id(&self, id: impl Id) -> Result<()> {
        self.data_source.delete_table_data_with_id(self, id).await
    }

    async fn update<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&mut E) + Send + Sync,
    {
        self.data_source.update_table_data(self, callback).await
    }

    async fn delete(&self) -> Result<()> {
        self.data_source.delete_table_data(self).await
    }
}
