use crate::dataset::Result;
use crate::entity::Entity;
use async_trait::async_trait;

/// DataSetSource trait for datasources that support dataset discovery
#[async_trait]
pub trait DataSetSource: Send + Sync {
    /// List all available datasets in this data source
    /// Returns a list of dataset names
    async fn list_candidates(&self) -> Result<Vec<String>>;

    /// Get metadata about a specific dataset
    async fn get_candidate_info(&self, name: &str) -> Result<Option<serde_json::Value>>;

    /// Check if a dataset exists in this source
    async fn has_dataset(&self, name: &str) -> Result<bool> {
        let candidates = self.list_candidates().await?;
        Ok(candidates.contains(&name.to_string()))
    }
}

/// Trait for datasources that can create readable datasets
/// Uses associated types to avoid dyn compatibility issues
#[async_trait]
pub trait ReadableDataSetSource: DataSetSource {
    /// The concrete readable dataset type this source creates
    type DataSet<E: Entity>: crate::dataset::ReadableDataSet<E> + Send + Sync;

    /// Create a readable dataset for the given name and entity type E
    async fn get_readable<E: Entity>(&self, name: &str) -> Result<Option<Self::DataSet<E>>>;
}

/// Trait for datasources that can create insertable datasets
#[async_trait]
pub trait InsertableDataSetSource: DataSetSource {
    /// The concrete insertable dataset type this source creates
    type DataSet<E: Entity>: crate::dataset::InsertableDataSet<E> + Send + Sync;

    /// Create an insertable dataset for the given name and entity type E
    async fn get_insertable<E: Entity>(&self, name: &str) -> Result<Option<Self::DataSet<E>>>;
}

/// Trait for datasources that can create writable datasets
#[async_trait]
pub trait WritableDataSetSource: DataSetSource {
    /// The concrete writable dataset type this source creates
    type DataSet<E: Entity>: crate::dataset::WritableDataSet<E> + Send + Sync;

    /// Create a writable dataset for the given name and entity type E
    async fn get_writable<E: Entity>(&self, name: &str) -> Result<Option<Self::DataSet<E>>>;
}
