use crate::dataset::{Id, Result, WritableDataSet, WritableValueSet};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use vantage_core::Entity;
use vantage_core::util::error::{Context, vantage_error};

use super::ImTable;

#[async_trait]
impl<E> WritableDataSet<E> for ImTable<E>
where
    E: Entity + Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    async fn insert_id(&self, id: impl Id, record: E) -> Result<()> {
        let id = id.into();
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if ID already exists
        if table.contains_key(&id) {
            return Err(vantage_error!("Record with id '{}' already exists", id));
        }

        let value = serde_json::to_value(record).context("Failed to serialize record")?;

        table.insert(id, value);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn replace_id(&self, id: impl Id, record: E) -> Result<()> {
        let id = id.into();
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        let value = serde_json::to_value(record).context("Failed to serialize record")?;

        table.insert(id, value);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn update<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&mut E) + Send + Sync,
    {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        for (_, value) in table.iter_mut() {
            if let Ok(mut record) = serde_json::from_value::<E>(value.clone()) {
                callback(&mut record);
                *value =
                    serde_json::to_value(record).context("Failed to serialize updated record")?;
            }
        }

        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }
}

#[async_trait]
impl<E> WritableValueSet for ImTable<E>
where
    E: Entity + Send + Sync + Clone + 'static,
{
    async fn insert_id_value(&self, id: &str, record: serde_json::Value) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if ID already exists
        if table.contains_key(id) {
            return Err(vantage_error!("Record with id '{}' already exists", id));
        }

        table.insert(id.to_string(), record);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn replace_id_value(&self, id: &str, record: serde_json::Value) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        table.insert(id.to_string(), record);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn patch_id(&self, id: &str, partial: serde_json::Value) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if record exists
        let existing_value = table
            .get(id)
            .ok_or_else(|| vantage_error!("Record with id '{}' not found", id))?
            .clone();

        // Merge the partial update with existing record
        let mut merged = existing_value;
        if let (serde_json::Value::Object(existing_obj), serde_json::Value::Object(partial_obj)) =
            (&mut merged, partial)
        {
            for (key, value) in partial_obj {
                existing_obj.insert(key, value);
            }
        } else {
            return Err(vantage_error!("Cannot patch non-object records"));
        }

        table.insert(id.to_string(), merged);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn delete_id(&self, id: &str) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        if table.shift_remove(id).is_none() {
            return Err(vantage_error!("Record with id '{}' not found", id));
        }

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
    use crate::dataset::ReadableDataSet;
    use crate::im::{ImDataSource, ImTable};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct User {
        id: Option<String>,
        name: String,
        email: String,
        age: u32,
    }

    #[tokio::test]
    async fn test_insert_id() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        let user = User {
            id: Some("user-1".to_string()),
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
            age: 35,
        };

        // First insert should succeed
        users.insert_id("user-1", user.clone()).await.unwrap();

        // Second insert with same ID should fail
        let result = users.insert_id("user-1", user).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_replace_id() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        let user1 = User {
            id: Some("user-1".to_string()),
            name: "Original".to_string(),
            email: "original@example.com".to_string(),
            age: 30,
        };

        let user2 = User {
            id: Some("user-1".to_string()),
            name: "Replaced".to_string(),
            email: "replaced@example.com".to_string(),
            age: 40,
        };

        // Replace on non-existent record (upsert)
        users.replace_id("user-1", user1).await.unwrap();

        // Replace on existing record
        users.replace_id("user-1", user2).await.unwrap();
    }

    #[tokio::test]
    async fn test_patch_id() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        let user = User {
            id: Some("user-1".to_string()),
            name: "Original".to_string(),
            email: "original@example.com".to_string(),
            age: 30,
        };

        users.insert_id("user-1", user).await.unwrap();

        // Patch existing record
        let patch = serde_json::json!({"name": "Patched", "age": 35});
        users.patch_id("user-1", patch).await.unwrap();

        // Patch non-existent record should fail
        let patch2 = serde_json::json!({"name": "Should Fail"});
        let result = users.patch_id("nonexistent", patch2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_id() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        let user = User {
            id: Some("user-1".to_string()),
            name: "ToDelete".to_string(),
            email: "delete@example.com".to_string(),
            age: 30,
        };

        users.insert_id("user-1", user).await.unwrap();

        // Delete record
        users.delete_id("user-1").await.unwrap();

        // Delete non-existent record should fail
        let result = users.delete_id("user-1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        users
            .insert_id(
                "user-1",
                User {
                    id: Some("user-1".to_string()),
                    name: "Alice".to_string(),
                    email: "alice@example.com".to_string(),
                    age: 30,
                },
            )
            .await
            .unwrap();

        users
            .insert_id(
                "user-2",
                User {
                    id: Some("user-2".to_string()),
                    name: "Bob".to_string(),
                    email: "bob@example.com".to_string(),
                    age: 25,
                },
            )
            .await
            .unwrap();

        // Update all records
        users
            .update(|user| {
                user.age += 1;
                user.name = format!("{} (Updated)", user.name);
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_delete_all() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        // Insert some records
        users
            .insert_id(
                "user-1",
                User {
                    id: Some("user-1".to_string()),
                    name: "Alice".to_string(),
                    email: "alice@example.com".to_string(),
                    age: 30,
                },
            )
            .await
            .unwrap();

        users
            .insert_id(
                "user-2",
                User {
                    id: Some("user-2".to_string()),
                    name: "Bob".to_string(),
                    email: "bob@example.com".to_string(),
                    age: 25,
                },
            )
            .await
            .unwrap();

        // Verify records exist
        let all_before = users.get().await.unwrap();
        assert_eq!(all_before.len(), 2);

        // Delete all
        users.delete_all().await.unwrap();

        // Verify all records are gone
        let all_after = users.get().await.unwrap();
        assert_eq!(all_after.len(), 0);
    }

    #[tokio::test]
    async fn test_insert_id_value() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        let user_value = serde_json::json!({
            "id": "user-1",
            "name": "Charlie",
            "email": "charlie@example.com",
            "age": 35
        });

        // First insert should succeed
        users
            .insert_id_value("user-1", user_value.clone())
            .await
            .unwrap();

        // Second insert with same ID should fail
        let result = users.insert_id_value("user-1", user_value).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_replace_id_value() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        let user1 = serde_json::json!({
            "id": "user-1",
            "name": "Original",
            "email": "original@example.com",
            "age": 30
        });

        let user2 = serde_json::json!({
            "id": "user-1",
            "name": "Replaced",
            "email": "replaced@example.com",
            "age": 40
        });

        // Replace on non-existent record (upsert)
        users.replace_id_value("user-1", user1).await.unwrap();

        // Replace on existing record
        users.replace_id_value("user-1", user2).await.unwrap();
    }

    #[tokio::test]
    async fn test_update_value() {
        let data_source = ImDataSource::new();
        let users = ImTable::<User>::new(&data_source, "users");

        users
            .insert_id_value(
                "user-1",
                serde_json::json!({
                    "id": "user-1",
                    "name": "Alice",
                    "email": "alice@example.com",
                    "age": 30
                }),
            )
            .await
            .unwrap();

        users
            .insert_id_value(
                "user-2",
                serde_json::json!({
                    "id": "user-2",
                    "name": "Bob",
                    "email": "bob@example.com",
                    "age": 25
                }),
            )
            .await
            .unwrap();

        // Update all records using JSON values
        users
            .update(|user| {
                user.age += 1;
                user.name = format!("{} (Updated)", user.name);
            })
            .await
            .unwrap();
    }
}
