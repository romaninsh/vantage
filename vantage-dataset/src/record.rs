use crate::traits::{ReadableDataSet, Result, WritableDataSet, WritableValueSet};
use std::ops::{Deref, DerefMut};
use vantage_core::util::error::vantage_error;
use vantage_types::{Entity, Record, TryFromRecord, TryIntoRecord};

/// A record represents a single entity with its ID, providing save functionality
pub struct ActiveEntity<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: TryIntoRecord<D::Value> + TryFromRecord<D::Value> + Send + Sync + Clone,
{
    id: D::Id,
    data: E,
    dataset: &'a D,
}

impl<'a, D, E> ActiveEntity<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: TryIntoRecord<D::Value> + TryFromRecord<D::Value> + Send + Sync + Clone,
{
    pub fn new(id: D::Id, data: E, dataset: &'a D) -> Self {
        Self { id, data, dataset }
    }

    /// Get the ID of this record
    pub fn id(&self) -> &D::Id {
        &self.id
    }

    /// Save the current state of the entity back to the dataset.
    ///
    /// This is a full **replace** (idempotent overwrite): it creates the row if
    /// missing and removes any fields absent from the in-memory copy. Matches
    /// [`ActiveRecord::save`], which also replaces.
    pub async fn save(&self) -> Result<E> {
        self.dataset.replace(self.id.clone(), &self.data).await
    }
}

impl<'a, D, E> ActiveEntity<'a, D, E>
where
    D: WritableDataSet<E> + WritableValueSet + ?Sized,
    E: Entity<D::Value> + Send + Sync + Clone,
{
    /// Delete this entity from the dataset.
    pub async fn delete(&self) -> Result<()> {
        self.dataset.delete(self.id.clone()).await
    }
}

impl<'a, D, E> ActiveEntity<'a, D, E>
where
    D: WritableDataSet<E> + ReadableDataSet<E> + ?Sized,
    E: Entity<D::Value> + Send + Sync + Clone,
{
    /// Re-fetch the entity from the dataset, replacing the in-memory copy.
    ///
    /// Errors if the row has been deleted by someone else since we loaded it.
    pub async fn reload(&mut self) -> Result<()> {
        let fresh = self
            .dataset
            .get(self.id.clone())
            .await?
            .ok_or_else(|| vantage_error!("reload: row not found"))?;
        self.data = fresh;
        Ok(())
    }
}

impl<'a, D, E> Deref for ActiveEntity<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: TryIntoRecord<D::Value> + TryFromRecord<D::Value> + Send + Sync + Clone,
{
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, D, E> DerefMut for ActiveEntity<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: TryIntoRecord<D::Value> + TryFromRecord<D::Value> + Send + Sync + Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

/// A wrapper for a data record represented by a Record, implementing save() method
/// for saving record into WritableValueSet after it's modified
pub struct ActiveRecord<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    id: D::Id,
    data: Record<D::Value>,
    dataset: &'a D,
}

impl<'a, D> ActiveRecord<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    pub fn new(id: D::Id, data: Record<D::Value>, dataset: &'a D) -> Self {
        Self { id, data, dataset }
    }

    /// Get the ID of this record
    pub fn id(&self) -> &D::Id {
        &self.id
    }

    /// Save the current state of the record back to the dataset.
    ///
    /// This is a full **replace** (idempotent overwrite), matching
    /// [`ActiveEntity::save`]: it creates the row if missing and drops any keys
    /// absent from the in-memory record. (Previously this patched — merging
    /// fields and never removing any, and erroring if the row had been deleted.)
    pub async fn save(&self) -> Result<Record<D::Value>> {
        self.dataset
            .replace_value(self.id.clone(), &self.data)
            .await
    }
}

impl<'a, D> Deref for ActiveRecord<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    type Target = Record<D::Value>;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, D> DerefMut for ActiveRecord<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}
