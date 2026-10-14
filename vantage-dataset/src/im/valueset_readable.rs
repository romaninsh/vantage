use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_types::Record;

use crate::{im::ImTable, traits::ReadableValueSet};

#[async_trait]
impl<E, V> ReadableValueSet for ImTable<E, V>
where
    V: Clone + Send + Sync + 'static,
    E: Send + Sync,
{
    async fn list_values(&self) -> crate::traits::Result<IndexMap<Self::Id, Record<Self::Value>>> {
        Ok(self
            .data_source
            .with_table(&self.table_name, |table| table.clone()))
    }

    async fn get_value(
        &self,
        id: impl Into<Self::Id> + Send,
    ) -> crate::traits::Result<Option<Record<Self::Value>>> {
        let id = id.into();
        Ok(self
            .data_source
            .with_table(&self.table_name, |table| table.get(&id).cloned()))
    }

    async fn get_some_value(
        &self,
    ) -> crate::traits::Result<Option<(Self::Id, Record<Self::Value>)>> {
        Ok(self.data_source.with_table(&self.table_name, |table| {
            table
                .iter()
                .next()
                .map(|(id, record)| (id.clone(), record.clone()))
        }))
    }
}
