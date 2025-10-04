// src/dataset/readable.rs

use super::{Id, Result};
use async_trait::async_trait;
use serde::de::DeserializeOwned;

#[async_trait]
pub trait ReadableDataSet<E> {
    async fn get(&self) -> Result<Vec<E>>;
    async fn get_some(&self) -> Result<Option<E>>;
    async fn get_id(&self, id: impl Id) -> Result<E>;

    // Get data as serde_json::Value for generic operations like indexing
    async fn get_values(&self) -> Result<Vec<serde_json::Value>>;

    // Generic methods for any deserializable type
    async fn get_as<T>(&self) -> Result<Vec<T>>
    where
        T: DeserializeOwned;

    async fn get_some_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned;
}
