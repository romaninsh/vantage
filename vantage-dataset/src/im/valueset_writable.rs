use async_trait::async_trait;
use vantage_types::Record;

use crate::{im::ImTable, traits::WritableValueSet};

#[async_trait]
impl<E, V> WritableValueSet for ImTable<E, V>
where
    V: Clone + Send + Sync + 'static,
    E: Send + Sync,
{
    async fn insert_value(
        &self,
        id: impl Into<Self::Id> + Send,
        record: &Record<Self::Value>,
    ) -> crate::traits::Result<Record<Self::Value>> {
        let id = id.into();
        let stored = self.data_source.with_table_mut(&self.table_name, |table| {
            if let Some(existing_record) = table.get(&id) {
                return existing_record.clone();
            }

            table.insert(id.clone(), record.clone());
            record.clone()
        });

        Ok(stored)
    }

    async fn replace_value(
        &self,
        id: impl Into<Self::Id> + Send,
        record: &Record<Self::Value>,
    ) -> crate::traits::Result<Record<Self::Value>> {
        let id = id.into();
        self.data_source.with_table_mut(&self.table_name, |table| {
            table.insert(id.clone(), record.clone());
        });

        Ok(record.clone())
    }

    async fn patch_value(
        &self,
        id: impl Into<Self::Id> + Send,
        partial: &Record<Self::Value>,
    ) -> crate::traits::Result<Record<Self::Value>> {
        let id = id.into();
        self.data_source.with_table_mut(&self.table_name, |table| {
            let mut existing_record = table
                .get(&id)
                .ok_or_else(|| {
                    vantage_core::error!("Record not found", id = id.clone()).mark_not_found()
                })?
                .clone();

            for (key, value) in partial.iter() {
                existing_record.insert(key.clone(), value.clone());
            }

            table.insert(id.clone(), existing_record.clone());

            Ok(existing_record)
        })
    }

    async fn delete(&self, id: impl Into<Self::Id> + Send) -> crate::traits::Result<()> {
        let id = id.into();
        self.data_source.with_table_mut(&self.table_name, |table| {
            table.shift_remove(&id);
        });
        Ok(())
    }

    async fn delete_all(&self) -> crate::traits::Result<()> {
        self.data_source
            .with_table_mut(&self.table_name, |table| table.clear());
        Ok(())
    }
}
