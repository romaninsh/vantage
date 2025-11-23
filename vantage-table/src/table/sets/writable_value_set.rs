use async_trait::async_trait;
use vantage_core::Result;
use vantage_dataset::WritableValueSet;
use vantage_types::{Entity, Record};

use crate::{prelude::TableSource, table::Table};

// Implement WritableValueSet by delegating to data source
#[async_trait]
impl<T: TableSource, E: Entity<T::Value>> WritableValueSet for Table<T, E> {
    async fn insert_value(
        &self,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.data_source()
            .insert_table_value(&self, id, record)
            .await
    }

    async fn replace_value(
        &self,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.data_source()
            .replace_table_value(&self, id, record)
            .await
    }

    async fn patch_value(
        &self,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>> {
        self.data_source()
            .patch_table_value(&self, id, partial)
            .await
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
    use crate::mocks::tablesource::MockTableSource;
    use serde_json::json;
    use vantage_dataset::ReadableValueSet;
    use vantage_types::{EmptyEntity, Record};

    #[tokio::test]
    async fn test_writable_value_set_implementation() {
        // Setup mock data
        let mock_data = vec![
            json!({"id": "1", "name": "Alice", "age": 30}),
            json!({"id": "2", "name": "Bob", "age": 25}),
        ];

        let mock_source = MockTableSource::new()
            .with_im_table("test_table", mock_data)
            .await;
        let table = Table::<MockTableSource, EmptyEntity>::new("test_table", mock_source);

        // Test insert_value with new ID
        let new_record = Record::from(json!({"name": "Charlie", "age": 35}));
        let inserted = table
            .insert_value(&"3".to_string(), &new_record)
            .await
            .unwrap();
        assert_eq!(inserted["id"], json!("3"));
        assert_eq!(inserted["name"], json!("Charlie"));
        assert_eq!(inserted["age"], json!(35));

        // Test insert_value with existing ID should fail
        let duplicate_record = Record::from(json!({"name": "David", "age": 40}));
        let result = table
            .insert_value(&"1".to_string(), &duplicate_record)
            .await;
        assert!(result.is_err());

        // Test replace_value with existing ID
        let updated_record = Record::from(json!({"name": "Bob Updated", "age": 26}));
        let replaced = table
            .replace_value(&"2".to_string(), &updated_record)
            .await
            .unwrap();
        assert_eq!(replaced["id"], json!("2"));
        assert_eq!(replaced["name"], json!("Bob Updated"));
        assert_eq!(replaced["age"], json!(26));

        // Test replace_value with new ID (should create)
        let new_record2 = Record::from(json!({"name": "Eve", "age": 28}));
        let replaced2 = table
            .replace_value(&"4".to_string(), &new_record2)
            .await
            .unwrap();
        assert_eq!(replaced2["id"], json!("4"));
        assert_eq!(replaced2["name"], json!("Eve"));

        // Test patch_value
        let patch = Record::from(json!({"age": 31}));
        let patched = table.patch_value(&"1".to_string(), &patch).await.unwrap();
        assert_eq!(patched["name"], json!("Alice")); // Original name preserved
        assert_eq!(patched["age"], json!(31)); // Age updated

        // Test patch_value with non-existing ID should fail
        let patch2 = Record::from(json!({"age": 50}));
        let result2 = table.patch_value(&"999".to_string(), &patch2).await;
        assert!(result2.is_err());

        // Test delete
        table.delete(&"2".to_string()).await.unwrap();
        let result3 = table.get_value(&"2".to_string()).await;
        assert!(result3.is_err()); // Should be deleted

        // Test delete non-existing ID should fail
        let result4 = table.delete(&"999".to_string()).await;
        assert!(result4.is_err());

        // Test delete_all
        table.delete_all().await.unwrap();
        let all_values = table.list_values().await.unwrap();
        assert_eq!(all_values.len(), 0); // All records should be deleted
    }
}
