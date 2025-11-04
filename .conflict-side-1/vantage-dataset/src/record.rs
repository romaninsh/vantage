use crate::dataset::{Id, ReadableDataSet, Result, WritableDataSet};
use async_trait::async_trait;
use std::ops::{Deref, DerefMut};
use vantage_core::Entity;

/// A record represents a single entity with its ID, providing save functionality
pub struct Record<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: Entity,
{
    id: String,
    data: E,
    dataset: &'a D,
}

impl<'a, D, E> Record<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: Entity + Clone,
{
    pub fn new(id: impl Id, data: E, dataset: &'a D) -> Self {
        Self {
            id: id.into(),
            data,
            dataset,
        }
    }

    /// Get the ID of this record
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Save the current state of the record back to the dataset
    pub async fn save(&self) -> Result<()> {
        self.dataset.replace_id(&self.id, self.data.clone()).await
    }
}

impl<'a, D, E> Deref for Record<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: Entity,
{
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, D, E> DerefMut for Record<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: Entity,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

/// Extension trait for datasets that support both reading and writing to provide record functionality
#[async_trait]
pub trait RecordDataSet<E>: ReadableDataSet<E> + WritableDataSet<E>
where
    E: Entity,
{
    async fn get_record(&self, id: impl Id) -> Result<Option<Record<'_, Self, E>>> {
        let id_str = id.into();
        match self.get_id(&id_str).await {
            Ok(data) => Ok(Some(Record::new(id_str, data, self))),
            Err(_) => Ok(None),
        }
    }
}

// Auto-implement for any type that has both readable and writable traits
impl<T, E> RecordDataSet<E> for T
where
    T: ReadableDataSet<E> + WritableDataSet<E>,
    E: Entity,
{
}
