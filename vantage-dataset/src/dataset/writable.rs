// src/dataset/writable.rs

use super::{InsertableDataSet, Result};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

#[async_trait]
pub trait WritableDataSet<T>: InsertableDataSet<T>
where
    T: Serialize + DeserializeOwned + Send,
{
    /// Update records using a callback that modifies each record in place
    async fn update<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&mut T) + Send + Sync;

    /// Delete all records in the DataSet
    async fn delete(&self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::super::DataSetError;
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct User {
        pub id: u64,
        pub name: String,
    }

    struct MockDataSet {
        data: std::sync::Mutex<Vec<serde_json::Value>>,
    }

    impl MockDataSet {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(vec![
                    serde_json::json!({"id": 1, "name": "Alice"}),
                    serde_json::json!({"id": 2, "name": "Bob"}),
                ]),
            }
        }

        fn len(&self) -> usize {
            self.data.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl InsertableDataSet<User> for MockDataSet {
        async fn insert(&self, record: User) -> Result<()> {
            let value = serde_json::to_value(record)
                .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))?;
            self.data.lock().unwrap().push(value);
            Ok(())
        }
    }

    #[async_trait]
    impl WritableDataSet<User> for MockDataSet {
        async fn update<F>(&self, callback: F) -> Result<()>
        where
            F: Fn(&mut User) + Send + Sync,
        {
            let mut data = self.data.lock().unwrap();
            for value in data.iter_mut() {
                if let Ok(mut record) = serde_json::from_value::<User>(value.clone()) {
                    callback(&mut record);
                    *value = serde_json::to_value(record)
                        .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))?;
                }
            }
            Ok(())
        }

        async fn delete(&self) -> Result<()> {
            self.data.lock().unwrap().clear();
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_update_user() {
        let dataset = MockDataSet::new();

        let result = dataset
            .update(|user| {
                user.name = format!("{} (Updated)", user.name);
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete() {
        let dataset = MockDataSet::new();
        assert!(dataset.len() > 0);

        let result = dataset.delete().await;
        assert!(result.is_ok());
        assert_eq!(dataset.len(), 0);
    }
}
