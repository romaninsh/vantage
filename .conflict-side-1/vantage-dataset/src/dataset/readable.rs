// src/dataset/readable.rs

use super::{Id, Result};
use async_trait::async_trait;
use vantage_core::Entity;

#[async_trait]
pub trait ReadableDataSet<E> {
    async fn get(&self) -> Result<Vec<E>>;
    async fn get_some(&self) -> Result<Option<E>>;
    async fn get_id(&self, id: impl Id) -> Result<E>;
}

#[async_trait]
pub trait ReadableValueSet {
    async fn get_values(&self) -> Result<Vec<serde_json::Value>>;
    async fn get_id_value(&self, id: &str) -> Result<serde_json::Value>;
    async fn get_some_value(&self) -> Result<Option<serde_json::Value>>;
}

#[async_trait]
pub trait ReadableAsDataSet {
    async fn get_as<T>(&self) -> Result<Vec<T>>
    where
        T: Entity;

    async fn get_id_as<T>(&self, id: &str) -> Result<T>
    where
        T: Entity;

    async fn get_some_as<T>(&self) -> Result<Option<T>>
    where
        T: Entity;
}
