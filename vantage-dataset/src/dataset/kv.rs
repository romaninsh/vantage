// src/dataset/indexable.rs

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

#[async_trait]
pub trait KvDataSet {
    type Key: Send + Sync;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get a record by its key/identifier
    async fn get_by_key<T>(&self, key: Self::Key) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned + Send;

    /// Set/update a record with a specific key
    async fn set_by_key<T>(&self, key: Self::Key, record: T) -> Result<(), Self::Error>
    where
        T: Serialize + Send;

    /// Delete a record by its key
    async fn delete_by_key(&self, key: Self::Key) -> Result<bool, Self::Error>;

    /// Check if a key exists
    async fn exists(&self, key: Self::Key) -> Result<bool, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::super::DataSetError;
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct User {
        pub id: u64,
        pub name: String,
    }

    struct MockKeyValueStore {
        data: std::sync::Mutex<HashMap<String, serde_json::Value>>,
    }

    impl MockKeyValueStore {
        fn new() -> Self {
            let mut initial_data = HashMap::new();
            initial_data.insert(
                "user:1".to_string(),
                serde_json::json!({"id": 1, "name": "Alice"}),
            );
            initial_data.insert(
                "user:2".to_string(),
                serde_json::json!({"id": 2, "name": "Bob"}),
            );

            Self {
                data: std::sync::Mutex::new(initial_data),
            }
        }

        fn len(&self) -> usize {
            self.data.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl KvDataSet for MockKeyValueStore {
        type Key = String;
        type Error = DataSetError;

        async fn get_by_key<T>(&self, key: Self::Key) -> Result<Option<T>, Self::Error>
        where
            T: DeserializeOwned + Send,
        {
            let data = self.data.lock().unwrap();
            match data.get(&key) {
                Some(value) => {
                    let record = serde_json::from_value(value.clone()).map_err(|e| {
                        DataSetError::other(format!("Deserialization error: {}", e))
                    })?;
                    Ok(Some(record))
                }
                None => Ok(None),
            }
        }

        async fn set_by_key<T>(&self, key: Self::Key, record: T) -> Result<(), Self::Error>
        where
            T: Serialize + Send,
        {
            let value = serde_json::to_value(record)
                .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))?;
            self.data.lock().unwrap().insert(key, value);
            Ok(())
        }

        async fn delete_by_key(&self, key: Self::Key) -> Result<bool, Self::Error> {
            Ok(self.data.lock().unwrap().remove(&key).is_some())
        }

        async fn exists(&self, key: Self::Key) -> Result<bool, Self::Error> {
            Ok(self.data.lock().unwrap().contains_key(&key))
        }
    }

    #[tokio::test]
    async fn test_get_by_key() {
        let store = MockKeyValueStore::new();

        // Get existing user
        let user: Option<User> = store.get_by_key("user:1".to_string()).await.unwrap();
        assert!(user.is_some());
        let user = user.unwrap();
        assert_eq!(user.id, 1);
        assert_eq!(user.name, "Alice");

        // Get non-existent user
        let missing: Option<User> = store.get_by_key("user:999".to_string()).await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_set_by_key() {
        let store = MockKeyValueStore::new();
        let initial_len = store.len();

        let new_user = User {
            id: 3,
            name: "Charlie".to_string(),
        };

        // Set new user
        let result = store.set_by_key("user:3".to_string(), new_user).await;
        assert!(result.is_ok());
        assert_eq!(store.len(), initial_len + 1);

        // Verify it was stored correctly
        let stored: Option<User> = store.get_by_key("user:3".to_string()).await.unwrap();
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().name, "Charlie");
    }

    #[tokio::test]
    async fn test_update_by_key() {
        let store = MockKeyValueStore::new();

        let updated_user = User {
            id: 1,
            name: "Alice Updated".to_string(),
        };

        // Update existing user
        let result = store.set_by_key("user:1".to_string(), updated_user).await;
        assert!(result.is_ok());

        // Verify update
        let user: Option<User> = store.get_by_key("user:1".to_string()).await.unwrap();
        assert_eq!(user.unwrap().name, "Alice Updated");
    }

    #[tokio::test]
    async fn test_delete_by_key() {
        let store = MockKeyValueStore::new();
        let initial_len = store.len();

        // Delete existing key
        let deleted = store.delete_by_key("user:1".to_string()).await.unwrap();
        assert!(deleted);
        assert_eq!(store.len(), initial_len - 1);

        // Try to delete non-existent key
        let not_deleted = store.delete_by_key("user:999".to_string()).await.unwrap();
        assert!(!not_deleted);
    }

    #[tokio::test]
    async fn test_exists() {
        let store = MockKeyValueStore::new();

        // Check existing key
        let exists = store.exists("user:1".to_string()).await.unwrap();
        assert!(exists);

        // Check non-existent key
        let not_exists = store.exists("user:999".to_string()).await.unwrap();
        assert!(!not_exists);
    }

    #[tokio::test]
    async fn test_generic_types() {
        let store = MockKeyValueStore::new();

        #[derive(Serialize, Deserialize)]
        struct Settings {
            theme: String,
            notifications: bool,
        }

        let settings = Settings {
            theme: "dark".to_string(),
            notifications: true,
        };

        // Store and retrieve generic type
        store
            .set_by_key("settings:user1".to_string(), settings)
            .await
            .unwrap();
        let retrieved: Option<Settings> = store
            .get_by_key("settings:user1".to_string())
            .await
            .unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.theme, "dark");
        assert!(retrieved.notifications);
    }
}
