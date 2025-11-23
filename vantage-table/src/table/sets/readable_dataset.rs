use async_trait::async_trait;
use indexmap::IndexMap;

use vantage_core::Result;
use vantage_dataset::prelude::ReadableDataSet;
use vantage_types::Entity;

use crate::{table::Table, traits::table_source::TableSource};

// Implement ReadableDataSet by delegating to data source and converting Records to Entities
#[async_trait]
impl<T, E> ReadableDataSet<E> for Table<T, E>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    async fn list(&self) -> Result<IndexMap<Self::Id, E>> {
        let records = self.data_source().list_table_values(&self).await?;
        let mut entities = IndexMap::new();

        for (id, record) in records {
            let entity: E = E::try_from_record(&record)
                .map_err(|_| vantage_core::error!("Failed to convert record to entity"))?;
            entities.insert(id, entity);
        }

        Ok(entities)
    }

    async fn get(&self, id: &Self::Id) -> Result<E> {
        let record = self.data_source().get_table_value(&self, id).await?;
        E::try_from_record(&record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))
    }

    async fn get_some(&self) -> Result<Option<(Self::Id, E)>> {
        if let Some((id, record)) = self.data_source().get_table_some_value(&self).await? {
            let entity: E = E::try_from_record(&record)
                .map_err(|_| vantage_core::error!("Failed to convert record to entity"))?;
            Ok(Some((id, entity)))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::tablesource::MockTableSource;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TestUser {
        id: Option<String>,
        name: String,
        age: i32,
    }

    #[tokio::test]
    async fn test_readable_dataset_implementation() {
        // Setup mock data
        let mock_data = vec![
            json!({"id": "1", "name": "Alice", "age": 30}),
            json!({"id": "2", "name": "Bob", "age": 25}),
            json!({"id": "3", "name": "Charlie", "age": 35}),
        ];

        let mock_source = MockTableSource::new()
            .with_im_table("test_table", mock_data)
            .await;
        let table = Table::<MockTableSource, TestUser>::new("test_table", mock_source);

        // Test list()
        let all_entities = table.list().await.unwrap();
        assert_eq!(all_entities.len(), 3);

        // Check that we have the expected IDs
        assert!(all_entities.contains_key("1"));
        assert!(all_entities.contains_key("2"));
        assert!(all_entities.contains_key("3"));

        // Check that entities contain expected data
        let entity_1 = &all_entities["1"];
        assert_eq!(entity_1.id, Some("1".to_string()));
        assert_eq!(entity_1.name, "Alice");
        assert_eq!(entity_1.age, 30);

        // Test get() with existing ID
        let entity_2 = table.get(&"2".to_string()).await.unwrap();
        assert_eq!(entity_2.id, Some("2".to_string()));
        assert_eq!(entity_2.name, "Bob");
        assert_eq!(entity_2.age, 25);

        // Test get() with non-existing ID
        let result = table.get(&"999".to_string()).await;
        assert!(result.is_err());

        // Test get_some()
        let some_entity = table.get_some().await.unwrap();
        assert!(some_entity.is_some());

        let (id, entity) = some_entity.unwrap();
        assert_eq!(id, "1"); // Should be the first record
        assert_eq!(entity.id, Some("1".to_string()));
        assert_eq!(entity.name, "Alice");

        // Test get_some() with empty table
        let empty_source = MockTableSource::new()
            .with_im_table("empty_table", vec![])
            .await;
        let empty_table = Table::<MockTableSource, TestUser>::new("empty_table", empty_source);
        let empty_result = empty_table.get_some().await.unwrap();
        assert!(empty_result.is_none());
    }

    #[tokio::test]
    async fn test_entity_conversion_errors() {
        // Setup mock data with invalid data for entity conversion
        let invalid_mock_data = vec![
            json!({"id": "1", "name": "Alice"}), // missing age field
        ];

        let mock_source = MockTableSource::new()
            .with_im_table("invalid_table", invalid_mock_data)
            .await;
        let table = Table::<MockTableSource, TestUser>::new("invalid_table", mock_source);

        // Test that conversion errors are properly handled
        let result = table.list().await;
        assert!(result.is_err());

        let result = table.get(&"1".to_string()).await;
        assert!(result.is_err());

        let result = table.get_some().await;
        assert!(result.is_err());
    }
}
