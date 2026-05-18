use std::time::SystemTime;

use ciborium::Value as CborValue;
use vantage_types::Record;

/// What a Scenery hands to UI code.
///
/// Preserves the underlying [`Record<CborValue>`] and layers display
/// metadata on top. The `dirty_fields` slot supports form-edit flows
/// where only some columns carry unsaved changes.
#[derive(Debug, Clone)]
pub struct EnrichedRecord {
    pub record: Record<CborValue>,
    pub status: RowStatus,
    pub dirty_fields: Option<Vec<String>>,
    pub fetched_at: Option<SystemTime>,
}

impl EnrichedRecord {
    /// Wrap a freshly-loaded cache row. Status is `Fresh`, no dirty
    /// fields, `fetched_at = now`.
    pub fn fresh(record: Record<CborValue>) -> Self {
        Self {
            record,
            status: RowStatus::Fresh,
            dirty_fields: None,
            fetched_at: Some(SystemTime::now()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RowStatus {
    Fresh,
    Stale,
    Loading,
    PendingWrite,
    WriteFailed { error: String },
    NotFound,
}
