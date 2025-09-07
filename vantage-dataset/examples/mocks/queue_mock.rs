// examples/queue_mock.rs

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use vantage_dataset::dataset::{DataSetError, InsertableDataSet, Result};

/// MockQueue collects all messages from all topics
#[derive(Debug, Clone)]
pub struct MockQueue {
    // topic_name -> Vec<messages>
    topics: Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>>,
}

impl MockQueue {
    pub fn init() -> Self {
        Self {
            topics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn message_count(&self, topic_name: &str) -> usize {
        self.topics
            .lock()
            .unwrap()
            .get(topic_name)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    pub fn get_messages(&self, topic_name: &str) -> Vec<serde_json::Value> {
        self.topics
            .lock()
            .unwrap()
            .get(topic_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_all_messages(&self) -> HashMap<String, Vec<serde_json::Value>> {
        self.topics.lock().unwrap().clone()
    }

    pub(crate) fn push_message(&self, topic_name: &str, message: serde_json::Value) {
        let mut topics = self.topics.lock().unwrap();
        topics
            .entry(topic_name.to_string())
            .or_insert_with(Vec::new)
            .push(message);
    }
}

/// Topic represents a typed topic in the queue
pub struct Topic<E> {
    queue: MockQueue,
    topic_name: String,
    _phantom: std::marker::PhantomData<E>,
}

impl<E> Topic<E>
where
    E: Serialize + Send,
{
    pub fn new(queue: &MockQueue) -> Self {
        // Use the type name as topic identifier
        let topic_name = std::any::type_name::<E>()
            .split("::")
            .last()
            .unwrap_or("unknown");
        Self {
            queue: queue.clone(),
            topic_name: topic_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<E> InsertableDataSet<E> for Topic<E>
where
    E: Serialize + Send + Sync,
{
    async fn insert(&self, record: E) -> Result<()> {
        let value = serde_json::to_value(record)
            .map_err(|e| DataSetError::other(format!("Serialization error: {}", e)))?;

        self.queue.push_message(&self.topic_name, value);
        Ok(())
    }
}
