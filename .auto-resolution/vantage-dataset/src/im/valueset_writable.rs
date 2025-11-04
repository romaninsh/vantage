use async_trait::async_trait;
use vantage_core::Entity;

use crate::{
    dataset::{Result, WritableValueSet},
    im::ImTable,
};

#[async_trait]
impl<E> WritableValueSet for ImTable<E>
where
    E: Entity,
{
    async fn insert_value(&self, id: &Self::Id, record: Self::Value) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if ID already exists - insert should be idempotent
        if table.contains_key(id) {
            return Ok(());
        }

        // Remove id field from the stored record since it's in the key
        let mut value = record;
        if let serde_json::Value::Object(ref mut map) = value {
            map.remove("id");
        }

        table.insert(id.clone(), value);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn replace_value(&self, id: &Self::Id, record: Self::Value) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Remove id field from the stored record since it's in the key
        let mut value = record;
        if let serde_json::Value::Object(ref mut map) = value {
            map.remove("id");
        }

        table.insert(id.clone(), value);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn patch_value(&self, id: &Self::Id, partial: Self::Value) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if record exists
        let existing_value = table
            .get(id)
            .ok_or_else(|| {
                vantage_core::util::error::vantage_error!("Record with id '{}' not found", id)
            })?
            .clone();

        // Merge the partial update with existing record
        let mut merged = existing_value;
        if let (serde_json::Value::Object(existing_obj), serde_json::Value::Object(partial_obj)) =
            (&mut merged, partial)
        {
            for (key, value) in partial_obj {
                if key != "id" {
                    // Don't allow patching the id field
                    existing_obj.insert(key, value);
                }
            }
        } else {
            return Err(vantage_core::util::error::vantage_error!(
                "Cannot patch non-object records"
            ));
        }

        table.insert(id.clone(), merged);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn delete(&self, id: &Self::Id) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Delete is idempotent - success even if record doesn't exist
        table.shift_remove(id);

        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
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
    async fn test_insert_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let value = serde_json::json!({"name": "Alice"});
        table
            .insert_value(&"user1".to_string(), value)
            .await
            .unwrap();

        // Second insert with same ID should be idempotent
        let value2 = serde_json::json!({"name": "Bob"});
        table
            .insert_value(&"user1".to_string(), value2)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_replace_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let value = serde_json::json!({"name": "Alice"});
        table
            .replace_value(&"user1".to_string(), value)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_patch_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        // Patch non-existent record should fail
        let patch = serde_json::json!({"name": "Updated"});
        let result = table.patch_value(&"nonexistent".to_string(), patch).await;
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
