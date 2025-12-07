use async_trait::async_trait;
use vantage_core::Result;
use vantage_dataset::prelude::InsertableValueSet;
use vantage_types::{Entity, Record};

use crate::{prelude::TableSource, table::Table};

// Implement InsertableValueSet by delegating to data source
#[async_trait]
impl<T: TableSource, E: Entity<T::Value>> InsertableValueSet for Table<T, E> {
    async fn insert_return_id_value(&self, record: &Record<Self::Value>) -> Result<Self::Id> {
        self.data_source()
            .insert_table_return_id_value(&self, record)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use serde_json::json;
    use vantage_types::EmptyEntity;

    #[tokio::test]
    async fn test_insertable_value_set_implementation() {
        let mock_source = MockTableSource::new().with_data("test_table", vec![]).await;
        let table = Table::<MockTableSource, EmptyEntity>::new("test_table", mock_source);

        // Test insert_return_id_value with no ID in record
        let new_record = Record::from(json!({"name": "Alice", "age": 30}));
        let id = table.insert_return_id_value(&new_record).await.unwrap();
        assert!(!id.is_empty());

        // Test insert_return_id_value with ID in record
        let record_with_id = Record::from(json!({"id": "user-123", "name": "Bob", "age": 25}));
        let id2 = table.insert_return_id_value(&record_with_id).await.unwrap();
        assert_eq!(id2, "user-123");

        // Test insert_return_id_value with null ID in record (should generate new ID)
        let record_with_null_id = Record::from(json!({"id": null, "name": "Charlie", "age": 35}));
        let id3 = table
            .insert_return_id_value(&record_with_null_id)
            .await
            .unwrap();
        assert!(!id3.is_empty());
        assert_ne!(id3, "null");

        // Test insert_return_id_value with empty string ID (should generate new ID)
        let record_with_empty_id = Record::from(json!({"id": "", "name": "David", "age": 40}));
        let id4 = table
            .insert_return_id_value(&record_with_empty_id)
            .await
            .unwrap();
        assert!(!id4.is_empty());
        assert_ne!(id4, "");

        // Test insert_return_id_value with numeric ID
        let record_with_numeric_id = Record::from(json!({"id": 42, "name": "Eve", "age": 28}));
        let id5 = table
            .insert_return_id_value(&record_with_numeric_id)
            .await
            .unwrap();
        assert_eq!(id5, "42");
    }
}
