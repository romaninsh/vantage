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
            // Check if record already exists (idempotent behavior)
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
            // Check if record exists
            let mut existing_record = table
                .get(&id)
                .ok_or_else(|| {
                    vantage_core::util::error::vantage_error!("Record with id '{}' not found", id)
                })?
                .clone();

            // Merge the partial fields into the existing record
            for (key, value) in partial.iter() {
                existing_record.insert(key.clone(), value.clone());
            }

            table.insert(id.clone(), existing_record.clone());

            Ok(existing_record)
        })
    }

    async fn delete(&self, id: impl Into<Self::Id> + Send) -> crate::traits::Result<()> {
        let id = id.into();
        // Delete is idempotent - success even if record doesn't exist
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im::ImDataSource;
    use serde::{Deserialize, Serialize};
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct User {
        id: Option<String>,
        name: String,
    }

    #[tokio::test]
    async fn test_replace_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let mut record = Record::new();
        record.insert(
            "name".to_string(),
            serde_json::Value::String("Alice".to_string()),
        );
        table.replace_value("user1", &record).await.unwrap();
    }

    #[tokio::test]
    async fn test_patch_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        // Patch non-existent record should fail
        let mut patch = Record::new();
        patch.insert(
            "name".to_string(),
            serde_json::Value::String("Updated".to_string()),
        );
        let result = table.patch_value("nonexistent", &patch).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        // Delete non-existent record should succeed (idempotent)
        table.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        table.delete_all().await.unwrap();
    }
}
