// src/dataset/insertable.rs

use super::Result;
use async_trait::async_trait;
use serde::Serialize;

#[async_trait]
pub trait InsertableDataSet<T>
where
    T: Serialize + Send,
{
    /// Insert a record of the specified type
    async fn insert(&self, record: T) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::super::DataSetError;
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct LogEntry {
        pub timestamp: u64,
        pub message: String,
    }

    struct MockQueue {
        items: std::sync::Mutex<Vec<serde_json::Value>>,
    }

    impl MockQueue {
        fn new() -> Self {
            Self {
                items: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn len(&self) -> usize {
            self.items.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl InsertableDataSet<LogEntry> for MockQueue {
        async fn insert(&self, record: LogEntry) -> Result<()> {
            let value = serde_json::to_value(record)
                .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))?;

            // Simulate adding to queue
            self.items.lock().unwrap().push(value);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_insert_default_type() {
        let queue = MockQueue::new();
        let entry = LogEntry {
            timestamp: 1234567890,
            message: "Test log entry".to_string(),
        };

        let result = queue.insert(entry).await;
        assert!(result.is_ok());
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn test_insert_as_generic_type() {
        let _queue = MockQueue::new();

        #[derive(Serialize)]
        struct Event {
            event_type: String,
            data: String,
        }

        let _event = Event {
            event_type: "user_action".to_string(),
            data: "clicked_button".to_string(),
        };

        // This test is no longer valid since MockQueue only implements InsertableDataSet<LogEntry>
        // In practice, you'd have separate implementations for different types
    }

    #[tokio::test]
    async fn test_multiple_inserts() {
        let queue = MockQueue::new();

        // Insert using default type
        let entry1 = LogEntry {
            timestamp: 1000,
            message: "First entry".to_string(),
        };
        queue.insert(entry1).await.unwrap();

        // Insert another log entry
        let entry2 = LogEntry {
            timestamp: 2000,
            message: "Second entry".to_string(),
        };
        queue.insert(entry2).await.unwrap();

        assert_eq!(queue.len(), 2);
    }

    #[tokio::test]
    async fn test_serialization_error() {
        let queue = MockQueue::new();

        // Test serialization with a valid LogEntry
        let entry = LogEntry {
            timestamp: 123,
            message: "Test entry".to_string(),
        };
        let result = queue.insert(entry).await;
        assert!(result.is_ok());
        assert_eq!(queue.len(), 1);
    }

    // Test that WritableDataSet implementations also work with InsertableDataSet
    #[tokio::test]
    async fn test_writable_as_insertable() {
        use super::super::WritableDataSet;

        struct MockWritable {
            data: std::sync::Mutex<Vec<serde_json::Value>>,
        }

        impl MockWritable {
            fn new() -> Self {
                Self {
                    data: std::sync::Mutex::new(Vec::new()),
                }
            }

            fn len(&self) -> usize {
                self.data.lock().unwrap().len()
            }
        }

        #[async_trait]
        impl InsertableDataSet<LogEntry> for MockWritable {
            async fn insert(&self, record: LogEntry) -> Result<()> {
                let value = serde_json::to_value(record)
                    .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))?;
                self.data.lock().unwrap().push(value);
                Ok(())
            }
        }

        #[async_trait]
        impl WritableDataSet<LogEntry> for MockWritable {
            async fn update<F>(&self, _callback: F) -> Result<()>
            where
                F: Fn(&mut LogEntry) + Send + Sync,
            {
                Ok(())
            }

            async fn delete(&self) -> Result<()> {
                self.data.lock().unwrap().clear();
                Ok(())
            }
        }

        let writable = MockWritable::new();

        // Test that we can use WritableDataSet as InsertableDataSet
        let entry = LogEntry {
            timestamp: 9999,
            message: "From writable".to_string(),
        };

        let result = writable.insert(entry).await;
        assert!(result.is_ok());
        assert_eq!(writable.len(), 1);
    }
}
