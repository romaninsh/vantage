use crate::dataset::{Result, WritableDataSet, WritableValueSet};
use std::ops::{Deref, DerefMut};
use vantage_core::Entity;

/// A record represents a single entity with its ID, providing save functionality
pub struct Record<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: Entity,
{
    id: D::Id,
    data: E,
    dataset: &'a D,
}

impl<'a, D, E> Record<'a, D, E>
where
    D: WritableDataSet<E> + ?Sized,
    E: Entity + Clone,
{
    pub fn new(id: D::Id, data: E, dataset: &'a D) -> Self {
        Self { id, data, dataset }
    }

    /// Get the ID of this record
    pub fn id(&self) -> &D::Id {
        &self.id
    }

    /// Save the current state of the record back to the dataset
    pub async fn save(&self) -> Result<()> {
        self.dataset.replace(&self.id, self.data.clone()).await
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

/// A wrapper for a data record represented by a Value, implementing save() method
/// for saving record into WritableValueSet after it's modified
pub struct RecordValue<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    id: D::Id,
    data: D::Value,
    dataset: &'a D,
}

impl<'a, D> RecordValue<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    pub fn new(id: D::Id, data: D::Value, dataset: &'a D) -> Self {
        Self { id, data, dataset }
    }

    /// Get the ID of this record
    pub fn id(&self) -> &D::Id {
        &self.id
    }

    /// Save the current state of the record back to the dataset
    pub async fn save(&self) -> Result<()> {
        self.dataset.patch_value(&self.id, self.data.clone()).await
    }
}

impl<'a, D> Deref for RecordValue<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    type Target = D::Value;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, D> DerefMut for RecordValue<'a, D>
where
    D: WritableValueSet + ?Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}
