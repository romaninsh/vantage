use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_types::{Entity, Record};

use crate::{table::Table, traits::table_source::TableSource};

// Implement ReadableValueSet by delegating to data source
#[async_trait]
impl<T: TableSource, E: Entity<T::Value>> ReadableValueSet for Table<T, E> {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        self.data_source().list_table_values(&self).await
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Record<Self::Value>> {
        self.data_source().get_table_value(&self, id).await
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        self.data_source().get_table_some_value(&self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::tablesource::MockTableSource;
    use serde_json::json;
    use vantage_types::EmptyEntity;

    #[tokio::test]
    async fn test_readable_value_set_implementation() {
        // Setup mock data
        let mock_data = vec![
            json!({"id": "1", "name": "Alice", "age": 30}),
            json!({"id": "2", "name": "Bob", "age": 25}),
            json!({"id": "3", "name": "Charlie", "age": 35}),
        ];

        let mock_source = MockTableSource::new()
            .with_im_table("test_table", mock_data)
            .await;
        let table = Table::<MockTableSource, EmptyEntity>::new("test_table", mock_source);

        // Test list_values
        let all_values = table.list_values().await.unwrap();
        assert_eq!(all_values.len(), 3);

        // Check that we have the expected IDs
        assert!(all_values.contains_key("1"));
        assert!(all_values.contains_key("2"));
        assert!(all_values.contains_key("3"));

        // Check that records contain expected data
        let record_1 = &all_values["1"];
        assert_eq!(record_1["name"], json!("Alice"));
        assert_eq!(record_1["age"], json!(30));

        // Test get_value with existing ID
        let value_2 = table.get_value(&"2".to_string()).await.unwrap();
        assert_eq!(value_2["name"], json!("Bob"));
        assert_eq!(value_2["age"], json!(25));

        // Test get_value with non-existing ID
        let result = table.get_value(&"999".to_string()).await;
        assert!(result.is_err());

        // Test get_some_value
        let some_value = table.get_some_value().await.unwrap();
        assert!(some_value.is_some());

        let (id, record) = some_value.unwrap();
        assert_eq!(id, "1"); // Should be the first record
        assert_eq!(record["name"], json!("Alice"));

        // Test get_some_value with empty table
        let empty_source = MockTableSource::new()
            .with_im_table("empty_table", vec![])
            .await;
        let empty_table = Table::<MockTableSource, EmptyEntity>::new("empty_table", empty_source);
        let empty_result = empty_table.get_some_value().await.unwrap();
        assert!(empty_result.is_none());
    }
}
