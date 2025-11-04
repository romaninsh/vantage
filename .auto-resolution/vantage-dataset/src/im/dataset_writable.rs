use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use vantage_core::Entity;

use crate::{
    dataset::{Result, WritableDataSet},
    im::ImTable,
};
use vantage_core::util::error::Context;

#[async_trait]
impl<E> WritableDataSet<E> for ImTable<E>
where
    E: Entity + Serialize + DeserializeOwned + Send + Sync,
{
    async fn insert(&self, id: &Self::Id, record: E) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if ID already exists - insert should be idempotent
        if table.contains_key(id) {
            return Ok(());
        }

        // Serialize record to JSON and remove id field since it's in the key
        let mut value = serde_json::to_value(record).context("Failed to serialize record")?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.remove("id");
        }

        table.insert(id.clone(), value);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn replace(&self, id: &Self::Id, record: E) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Serialize record to JSON and remove id field since it's in the key
        let mut value = serde_json::to_value(record).context("Failed to serialize record")?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.remove("id");
        }

        table.insert(id.clone(), value);
        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }

    async fn patch(&self, id: &Self::Id, partial: E) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);

        // Check if record exists
        let existing_value = table
            .get(id)
            .ok_or_else(|| {
                vantage_core::util::error::vantage_error!("Record with id '{}' not found", id)
            })?
            .clone();

        // Serialize partial record to get update fields
        let partial_value =
            serde_json::to_value(partial).context("Failed to serialize partial record")?;

        // Merge the partial update with existing record
        let mut merged = existing_value;
        if let (serde_json::Value::Object(existing_obj), serde_json::Value::Object(partial_obj)) =
            (&mut merged, partial_value)
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
    async fn test_insert() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let user = User {
            id: None,
            name: "Alice".to_string(),
        };
        table.insert(&"user1".to_string(), user).await.unwrap();

        // Second insert with same ID should be idempotent
        let user2 = User {
            id: None,
            name: "Bob".to_string(),
        };
        table.insert(&"user1".to_string(), user2).await.unwrap();
    }

    #[tokio::test]
    async fn test_replace() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let user = User {
            id: None,
            name: "Alice".to_string(),
        };
        table.replace(&"user1".to_string(), user).await.unwrap();
    }

    #[tokio::test]
    async fn test_patch() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        // Patch non-existent record should fail
        let user = User {
            id: None,
            name: "Alice".to_string(),
        };
        let result = table.patch(&"nonexistent".to_string(), user).await;
        assert!(result.is_err());
    }
}
