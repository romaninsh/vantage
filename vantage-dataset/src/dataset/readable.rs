// src/dataset/readable.rs

use super::Result;
use async_trait::async_trait;
use serde::de::DeserializeOwned;

#[async_trait]
pub trait ReadableDataSet<E> {
    async fn get(&self) -> Result<Vec<E>>;
    async fn get_some(&self) -> Result<Option<E>>;

    // Generic methods for any deserializable type
    async fn get_as<T>(&self) -> Result<Vec<T>>
    where
        T: DeserializeOwned;

    async fn get_some_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned;
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::super::DataSetError;
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct User {
        pub id: u64,
        pub name: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct VipUser {
        pub id: u64,
        pub name: String,
        #[serde(default = "default_vip_level")]
        pub vip_level: u8,
    }

    fn default_vip_level() -> u8 {
        1
    }

    struct UserSet<T> {
        data: Vec<serde_json::Value>,
        _marker: std::marker::PhantomData<T>,
    }

    impl<T> UserSet<T> {
        fn new() -> Self {
            let data = vec![
                serde_json::json!({
                    "id": 1,
                    "name": "Alice",
                    "vip_level": 3
                }),
                serde_json::json!({
                    "id": 2,
                    "name": "Bob"
                }),
            ];
            UserSet {
                data,
                _marker: PhantomData,
            }
        }
    }

    #[async_trait]
    impl<E: Sync + DeserializeOwned> ReadableDataSet<E> for UserSet<E> {
        async fn get(&self) -> Result<Vec<E>> {
            self.get_as().await
        }

        async fn get_some(&self) -> Result<Option<E>> {
            self.get_some_as().await
        }

        async fn get_as<T>(&self) -> Result<Vec<T>>
        where
            T: DeserializeOwned,
        {
            self.data
                .iter()
                .map(|v| {
                    serde_json::from_value(v.clone())
                        .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))
                })
                .collect()
        }

        async fn get_some_as<T>(&self) -> Result<Option<T>>
        where
            T: DeserializeOwned,
        {
            let Some(value) = self.data.first() else {
                return Ok(None);
            };
            serde_json::from_value(value.clone())
                .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))
        }
    }

    #[tokio::test]
    async fn test_get_user_type() {
        let user_set = UserSet::<User>::new();

        // Get as User type
        let user = user_set.get_some().await.unwrap().unwrap();
        assert_eq!(user.id, 1);
        assert_eq!(user.name, "Alice");

        // Get all as User type
        let users = user_set.get().await.unwrap();
        assert_eq!(users.len(), 2);
    }

    #[tokio::test]
    async fn test_get_vip_user() {
        let user_set = UserSet::<User>::new();

        // Get as VipUser type
        let vip_user: VipUser = user_set.get_some_as().await.unwrap().unwrap();
        assert_eq!(vip_user.id, 1);
        assert_eq!(vip_user.name, "Alice");
        assert_eq!(vip_user.vip_level, 3);

        // Get all as VipUser type
        let vip_users: Vec<VipUser> = user_set.get_as().await.unwrap();
        assert_eq!(vip_users.len(), 2);
        assert_eq!(vip_users[1].vip_level, 1); // Uses default for Bob
    }

    #[tokio::test]
    async fn test_get_json_value() {
        let user_set = UserSet::new();

        // Get as serde_json::Value using get_some
        let value: serde_json::Value = user_set.get_some().await.unwrap().unwrap();
        assert_eq!(value["id"], 1);
        assert_eq!(value["name"], "Alice");

        // Get all as serde_json::Value
        let values: Vec<serde_json::Value> = user_set.get().await.unwrap();
        assert_eq!(values.len(), 2);
    }

    #[tokio::test]
    async fn test_partial_deserialization() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct PartialUser {
            pub name: String,
            // Ignoring id field
        }

        let user_set = UserSet::new();
        let partial: PartialUser = user_set.get_some().await.unwrap().unwrap();
        assert_eq!(partial.name, "Alice");
    }
}
