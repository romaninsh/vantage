//! Record functionality for vantage-table
//!
//! This module provides Record support for Table<T, E> where T: TableSource.
//! Records allow individual entity instances to be loaded, modified, and saved back.

use crate::{Entity, Table, TableSource};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::ops::{Deref, DerefMut};
use vantage_dataset::dataset::{Id, ReadableDataSet, Result, WritableDataSet};

/// A record represents a single entity with its ID, providing save functionality
pub struct Record<'a, E, T>
where
    E: Entity,
    T: WritableDataSet<E> + ?Sized,
{
    id: String,
    data: E,
    table: &'a T,
}

impl<'a, E, T> Record<'a, E, T>
where
    E: Entity + Clone,
    T: WritableDataSet<E> + ?Sized,
{
    pub fn new(id: impl Id, data: E, table: &'a T) -> Self {
        Self {
            id: id.into(),
            data,
            table,
        }
    }

    /// Get the ID of this record
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Save the current state of the record back to the table
    pub async fn save(&self) -> Result<()>
    where
        E: Serialize + DeserializeOwned + Send + Sync,
    {
        self.table.replace_id(&self.id, self.data.clone()).await
    }
}

impl<'a, E, T> Deref for Record<'a, E, T>
where
    E: Entity,
    T: WritableDataSet<E> + ?Sized,
{
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, E, T> DerefMut for Record<'a, E, T>
where
    E: Entity,
    T: WritableDataSet<E> + ?Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

/// Extension trait for tables that support both reading and writing to provide record functionality
#[async_trait]
pub trait RecordTable<E>: ReadableDataSet<E> + WritableDataSet<E>
where
    E: Entity,
{
    async fn get_record(&self, id: impl Id) -> Result<Option<Record<'_, E, Self>>>;
}

#[async_trait]
impl<T, E> RecordTable<E> for Table<T, E>
where
    T: TableSource + Clone + Send + Sync,
    E: Entity + Serialize + DeserializeOwned + Send + Sync,
{
    async fn get_record(&self, id: impl Id) -> Result<Option<Record<'_, E, Table<T, E>>>> {
        let id_str = id.into();
        match self.get_id(&id_str).await {
            Ok(data) => Ok(Some(Record::new(id_str, data, self))),
            Err(_) => Ok(None),
        }
    }
}
