use async_trait::async_trait;
use vantage_types::{Entity, Record};

use crate::{im::ImTable, traits::WritableValueSet};

#[async_trait]
impl<E> WritableValueSet for ImTable<E>
where
    E: Entity,
{
    async fn insert_value(
        &self,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> crate::traits::Result<Record<Self::Value>> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if record already exists (idempotent behavior)
        if let Some(existing_record) = table.get(id) {
            return Ok(existing_record.clone());
        }

        table.insert(id.clone(), record.clone());
        self.data_source.update_table(&self.table_name, table);

        Ok(record.clone())
    }

    async fn replace_value(
        &self,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> crate::traits::Result<Record<Self::Value>> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        table.insert(id.clone(), record.clone());
        self.data_source.update_table(&self.table_name, table);

        Ok(record.clone())
    }

    async fn patch_value(
        &self,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> crate::traits::Result<Record<Self::Value>> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if record exists
        let mut existing_record = table
            .get(id)
            .ok_or_else(|| {
                vantage_core::util::error::vantage_error!("Record with id '{}' not found", id)
            })?
            .clone();

        // Merge the partial fields into the existing record
        for (key, value) in partial.iter() {
            existing_record.insert(key.clone(), value.clone());
        }

        table.insert(id.clone(), existing_record.clone());
        self.data_source.update_table(&self.table_name, table);

        Ok(existing_record)
    }

    async fn delete(&self, id: &Self::Id) -> crate::traits::Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Delete is idempotent - success even if record doesn't exist
        table.shift_remove(id);

        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn delete_all(&self) -> crate::traits::Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);
        table.clear();
        self.data_source.update_table(&self.table_name, table);
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
        table
            .replace_value(&"user1".to_string(), &record)
            .await
            .unwrap();
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
        let result = table.patch_value(&"nonexistent".to_string(), &patch).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        // Delete non-existent record should succeed (idempotent)
        table.delete(&"nonexistent".to_string()).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_all() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        table.delete_all().await.unwrap();
    }
}
