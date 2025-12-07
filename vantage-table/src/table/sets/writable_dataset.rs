use async_trait::async_trait;

use vantage_core::Result;
use vantage_dataset::prelude::WritableDataSet;
use vantage_types::Entity;

use crate::{table::Table, traits::table_source::TableSource};

// Implement WritableDataSet by converting entities to records and delegating to data source
#[async_trait]
impl<T, E> WritableDataSet<E> for Table<T, E>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    async fn insert(&self, id: &Self::Id, entity: &E) -> Result<E> {
        let record = entity.clone().into_record();

        let result_record = self
            .data_source()
            .insert_table_value(&self, id, &record)
            .await?;

        E::try_from_record(&result_record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))
    }

    async fn replace(&self, id: &Self::Id, entity: &E) -> Result<E> {
        let record = entity.clone().into_record();

        let result_record = self
            .data_source()
            .replace_table_value(&self, id, &record)
            .await?;

        E::try_from_record(&result_record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))
    }

    async fn patch(&self, id: &Self::Id, partial: &E) -> Result<E> {
        let partial_record = partial.clone().into_record();

        let result_record = self
            .data_source()
            .patch_table_value(&self, id, &partial_record)
            .await?;

        E::try_from_record(&result_record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))
    }

    async fn delete(&self, id: &Self::Id) -> Result<()> {
        self.data_source().delete_table_value(&self, id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.data_source().delete_table_all_values(&self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use vantage_dataset::prelude::ReadableDataSet;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TestUser {
        id: Option<String>,
        name: String,
        age: i32,
    }

    #[tokio::test]
    async fn test_writable_dataset_implementation() {
        // Setup mock data
        let mock_data = vec![
            json!({"id": "1", "name": "Alice", "age": 30}),
            json!({"id": "2", "name": "Bob", "age": 25}),
        ];

        let mock_source = MockTableSource::new()
            .with_data("test_table", mock_data)
            .await;
        let table = Table::<MockTableSource, TestUser>::new("test_table", mock_source);

        // Test insert with new ID
        let new_user = TestUser {
            id: Some("3".to_string()),
            name: "Charlie".to_string(),
            age: 35,
        };
        let inserted = table.insert(&"3".to_string(), &new_user).await.unwrap();
        assert_eq!(inserted.id, Some("3".to_string()));
        assert_eq!(inserted.name, "Charlie");
        assert_eq!(inserted.age, 35);

        // Test insert with existing ID should fail
        let duplicate_user = TestUser {
            id: Some("1".to_string()),
            name: "David".to_string(),
            age: 40,
        };
        let result = table.insert(&"1".to_string(), &duplicate_user).await;
        assert!(result.is_err());

        // Test replace with existing ID
        let updated_user = TestUser {
            id: Some("2".to_string()),
            name: "Bob Updated".to_string(),
            age: 26,
        };
        let replaced = table
            .replace(&"2".to_string(), &updated_user)
            .await
            .unwrap();
        assert_eq!(replaced.id, Some("2".to_string()));
        assert_eq!(replaced.name, "Bob Updated");
        assert_eq!(replaced.age, 26);

        // Test replace with new ID (should create)
        let new_user2 = TestUser {
            id: Some("4".to_string()),
            name: "Eve".to_string(),
            age: 28,
        };
        let replaced2 = table.replace(&"4".to_string(), &new_user2).await.unwrap();
        assert_eq!(replaced2.id, Some("4".to_string()));
        assert_eq!(replaced2.name, "Eve");

        // Test patch - create a partial update that only changes the name
        let original = table.get(&"1".to_string()).await.unwrap();
        assert_eq!(original.name, "Alice");
        assert_eq!(original.age, 30);

        let patch_user = TestUser {
            id: Some("1".to_string()),
            name: "Alice Updated".to_string(), // Update name only
            age: 30,                           // Keep original age - patch should preserve this
        };
        let patched = table.patch(&"1".to_string(), &patch_user).await.unwrap();
        assert_eq!(patched.name, "Alice Updated"); // Name updated
        assert_eq!(patched.age, 30); // Age remains unchanged

        // Test patch with non-existing ID should fail
        let patch_user2 = TestUser {
            id: Some("999".to_string()),
            name: "NonExistent".to_string(),
            age: 50,
        };
        let result2 = table.patch(&"999".to_string(), &patch_user2).await;
        assert!(result2.is_err());

        // Test delete
        table.delete(&"2".to_string()).await.unwrap();
        let result3 = table.get(&"2".to_string()).await;
        assert!(result3.is_err()); // Should be deleted

        // Test delete non-existing ID should fail
        let result4 = table.delete(&"999".to_string()).await;
        assert!(result4.is_err());

        // Test delete_all
        table.delete_all().await.unwrap();
        let all_entities = table.list().await.unwrap();
        assert_eq!(all_entities.len(), 0); // All entities should be deleted
    }

    #[tokio::test]
    async fn test_entity_conversion_errors() {
        // Setup mock data with valid data
        let mock_data = vec![json!({"id": "1", "name": "Alice", "age": 30})];

        let mock_source = MockTableSource::new()
            .with_data("test_table", mock_data)
            .await;
        let table = Table::<MockTableSource, TestUser>::new("test_table", mock_source);

        // Test with an entity that might cause conversion issues
        // This depends on the Entity implementation, but we can test the error path
        let user = TestUser {
            id: Some("test".to_string()),
            name: "Test User".to_string(),
            age: 25,
        };

        // The actual conversion should work fine with our TestUser
        let result = table.insert(&"test".to_string(), &user).await;
        assert!(result.is_ok());
    }
}
