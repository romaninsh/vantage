use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Represents a kind of set, that can be converted into a vector
/// of scalar values. This is useful for holding IDs, such as user IDs.
/// Scalar DataSet can be used to import foreign expressions (between
/// different data sources)
#[async_trait]
pub trait ScalarDataSet: Send + Sync {
    /// Fetch all records and return them as Vec. Value itself
    /// would likely be a string or number.
    async fn enumerate(&self) -> Result<Vec<Value>>;

    // /// Fetch all records and return them as a specific type.
    // /// This should work well with UUIDs or other custom
    // /// scalar types.
    // async fn enumerate_into<V>(&self) -> Result<Vec<V>>;
}
