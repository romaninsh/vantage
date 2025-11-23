use async_trait::async_trait;

use vantage_core::Result;
use vantage_dataset::prelude::InsertableDataSet;
use vantage_types::Entity;

use crate::{table::Table, traits::table_source::TableSource};

// Implement InsertableDataSet by converting entities to records and delegating to data source
#[async_trait]
impl<T, E> InsertableDataSet<E> for Table<T, E>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    async fn insert_return_id(&self, entity: &E) -> Result<Self::Id> {
        let record = entity.clone().into_record();

        self.data_source()
            .insert_table_return_id_value(&self, &record)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::tablesource::MockTableSource;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TestEvent {
        id: Option<String>,
        event_type: String,
        message: String,
        timestamp: Option<i64>,
    }

    #[tokio::test]
    async fn test_insertable_dataset_implementation() {
        let mock_source = MockTableSource::new().with_im_table("events", vec![]).await;
        let table = Table::<MockTableSource, TestEvent>::new("events", mock_source);

        // Test insert_return_id with entity without ID (should generate new ID)
        let event = TestEvent {
            id: None,
            event_type: "user_login".to_string(),
            message: "User logged in successfully".to_string(),
            timestamp: Some(1234567890),
        };
        let id = table.insert_return_id(&event).await.unwrap();
        assert!(!id.is_empty());

        // Test insert_return_id with entity with ID (should use provided ID)
        let event_with_id = TestEvent {
            id: Some("event-123".to_string()),
            event_type: "user_logout".to_string(),
            message: "User logged out".to_string(),
            timestamp: Some(1234567900),
        };
        let id2 = table.insert_return_id(&event_with_id).await.unwrap();
        assert_eq!(id2, "event-123");

        // Test insert_return_id with empty ID (should generate new ID)
        let event_with_empty_id = TestEvent {
            id: Some("".to_string()),
            event_type: "page_view".to_string(),
            message: "User viewed homepage".to_string(),
            timestamp: Some(1234567910),
        };
        let id3 = table.insert_return_id(&event_with_empty_id).await.unwrap();
        assert!(!id3.is_empty());
        assert_ne!(id3, "");

        // Test multiple inserts (each should get unique ID)
        let event1 = TestEvent {
            id: None,
            event_type: "click".to_string(),
            message: "Button clicked".to_string(),
            timestamp: Some(1234567920),
        };
        let event2 = TestEvent {
            id: None,
            event_type: "click".to_string(),
            message: "Button clicked".to_string(), // Same data
            timestamp: Some(1234567930),
        };

        let id4 = table.insert_return_id(&event1).await.unwrap();
        let id5 = table.insert_return_id(&event2).await.unwrap();

        // Should get different IDs even for same event data
        assert!(!id4.is_empty());
        assert!(!id5.is_empty());
        assert_ne!(id4, id5);
    }

    #[tokio::test]
    async fn test_entity_conversion_for_insert() {
        let mock_source = MockTableSource::new()
            .with_im_table("test_events", vec![])
            .await;
        let table = Table::<MockTableSource, TestEvent>::new("test_events", mock_source);

        // Test with minimal event data
        let minimal_event = TestEvent {
            id: None,
            event_type: "test".to_string(),
            message: "Test event".to_string(),
            timestamp: None,
        };

        let result = table.insert_return_id(&minimal_event).await;
        assert!(result.is_ok());

        let id = result.unwrap();
        assert!(!id.is_empty());
    }
}
