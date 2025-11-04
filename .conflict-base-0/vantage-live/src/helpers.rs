//! Helper functions for vantage-live

use vantage_core::Result;

/// Extract ID from entity by serializing and looking for "id" field
pub(crate) fn extract_id<E: serde::Serialize>(entity: &E) -> Result<String> {
    todo!()
}

/// Find differing fields between two JSON values
pub(crate) fn diff_fields(old: &serde_json::Value, new: &serde_json::Value) -> Vec<String> {
    todo!()
}
