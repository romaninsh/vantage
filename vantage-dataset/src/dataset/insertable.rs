use super::Result;
use async_trait::async_trait;
use serde::Serialize;

#[async_trait]
pub trait InsertableDataSet<T>
where
    T: Serialize + Send,
{
    /// Insert a record and return generated ID
    async fn insert(&self, record: T) -> Result<Option<String>>;
}

/// Trait for datasets that can import records from other datasets
#[async_trait]
pub trait Importable<T>
where
    T: Send,
{
    /// Import records from another dataset
    async fn import<D>(&mut self, source: D) -> Result<()>
    where
        D: crate::dataset::ReadableDataSet<T> + Send;
}
