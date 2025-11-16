use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Entity;
use vantage_core::Result;
use vantage_dataset::prelude::WritableValueSet;
use vantage_dataset::{prelude::ReadableValueSet, traits::ValueSet};
use vantage_types::Record;

use crate::{table::Table, traits::table_source::TableSource};

impl<T: TableSource, E: Entity> ValueSet for Table<T, E> {
    type Id = String;
    type Value = T::Value;
}

// Implement ReadableValueSet by delegating to data source
#[async_trait]
impl<T: TableSource, E: Entity> ReadableValueSet for Table<T, E> {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        // TODO: Implement using data source query capabilities
        todo!("Implement list_values using TableSource")
    }

    async fn get_value(&self, _id: &Self::Id) -> Result<Record<Self::Value>> {
        // TODO: Implement using data source query capabilities
        todo!("Implement get_value using TableSource")
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        // TODO: Implement using data source query capabilities
        todo!("Implement get_some_value using TableSource")
    }
}

// Implement WritableValueSet by delegating to data source
#[async_trait]
impl<T: TableSource, E: Entity> WritableValueSet for Table<T, E> {
    async fn insert_value(
        &self,
        _id: &Self::Id,
        _record: Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        // TODO: Implement using data source mutation capabilities
        todo!("Implement insert_value using TableSource")
    }

    async fn replace_value(
        &self,
        _id: &Self::Id,
        _record: Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        // TODO: Implement using data source mutation capabilities
        todo!("Implement replace_value using TableSource")
    }

    async fn patch_value(
        &self,
        _id: &Self::Id,
        _partial: Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        // TODO: Implement using data source mutation capabilities
        todo!("Implement patch_value using TableSource")
    }

    async fn delete(&self, _id: &Self::Id) -> Result<()> {
        // TODO: Implement using data source mutation capabilities
        todo!("Implement delete using TableSource")
    }

    async fn delete_all(&self) -> Result<()> {
        // TODO: Implement using data source mutation capabilities
        todo!("Implement delete_all using TableSource")
    }
}
