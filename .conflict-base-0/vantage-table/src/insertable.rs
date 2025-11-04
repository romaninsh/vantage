//! Insertable implementation for Table
//!
//! This module provides the InsertableDataSet trait implementation for Table<T, E>
//! where T: TableSource, allowing tables to insert records into their underlying data source.

use crate::{Entity, Table, TableSource};
use async_trait::async_trait;
use serde::Serialize;
use vantage_dataset::dataset::{InsertableDataSet, Result};

#[async_trait]
impl<T, E> InsertableDataSet<E> for Table<T, E>
where
    T: TableSource + Send + Sync,
    E: Entity + Serialize + Send + Sync,
{
    /// Insert a record and return generated ID
    async fn insert(&self, record: E) -> Result<Option<String>> {
        self.data_source.insert_table_data(self, record).await
    }
}
