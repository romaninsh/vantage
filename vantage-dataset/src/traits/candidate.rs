use std::collections::HashSet;

/// Capabilities that a dataset candidate might support
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataSetCapability {
    Readable,
    Insertable,
    Writable,
    Indexable, // Key-value style access
}

/// Represents a potential dataset that can be materialized
/// Simple trait without generics to enable trait objects
pub trait DataSetCandidate: Send + Sync {
    /// Get the name/identifier for this candidate
    fn name(&self) -> &str;

    /// Get the capabilities this candidate supports
    fn capabilities(&self) -> HashSet<DataSetCapability>;

    /// Check if candidate has specific capability
    fn has_capability(&self, capability: &DataSetCapability) -> bool {
        self.capabilities().contains(capability)
    }

    /// Clone this candidate (for trait object compatibility)
    fn clone_box(&self) -> Box<dyn DataSetCandidate>;

    /// Get metadata about this dataset candidate
    fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name(),
            "capabilities": self.capabilities().iter().map(|c| format!("{:?}", c)).collect::<Vec<_>>()
        })
    }
}

// Auto-implement Clone for Box<dyn DataSetCandidate>
impl Clone for Box<dyn DataSetCandidate> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
