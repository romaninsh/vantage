use super::{Id, Result};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

#[async_trait]
pub trait WritableDataSet<E>
where
    E: Serialize + DeserializeOwned + Send,
{
    /// Insert a record with a specific ID, fails if ID already exists
    async fn insert_id(&self, id: impl Id, record: E) -> Result<()>;

    /// Replace a record by ID (upsert - creates if missing, replaces if exists)
    async fn replace_id(&self, id: impl Id, record: E) -> Result<()>;

    /// Partially update a record by ID, fails if record doesn't exist
    async fn patch_id(&self, id: impl Id, partial: serde_json::Value) -> Result<()>;

    /// Delete a record by ID
    async fn delete_id(&self, id: impl Id) -> Result<()>;

    /// Update records using a callback that modifies each record in place
    async fn update<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(&mut E) + Send + Sync;

    /// Delete all records in the DataSet
    async fn delete(&self) -> Result<()>;
}
