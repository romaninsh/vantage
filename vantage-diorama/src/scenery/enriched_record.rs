use std::time::SystemTime;

use ciborium::Value as CborValue;
use vantage_types::Record;

/// What a Scenery hands to UI code.
///
/// Preserves the underlying [`Record<CborValue>`] and layers display
/// metadata on top.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EnrichedRecord {
    pub record: Record<CborValue>,
    pub status: RowStatus,
    pub fetched_at: Option<SystemTime>,
}

impl EnrichedRecord {
    /// Wrap a row with `status`, `fetched_at = now`.
    pub(crate) fn now(record: Record<CborValue>, status: RowStatus) -> Self {
        Self {
            record,
            status,
            fetched_at: Some(SystemTime::now()),
        }
    }

    /// Wrap a freshly-loaded cache row. Status is `Fresh`, `fetched_at = now`.
    pub fn fresh(record: Record<CborValue>) -> Self {
        Self::now(record, RowStatus::Fresh)
    }

    /// Wrap a row that has only been partially loaded by the list pass —
    /// its id and cheap columns are present, but the expensive detail
    /// columns are still pending hydration.
    pub fn incomplete(record: Record<CborValue>) -> Self {
        Self::now(record, RowStatus::Incomplete)
    }

    /// Wrap a row carrying an **optimistically-staged** write — the new value
    /// is shown immediately while the write-through is in flight.
    pub fn pending_write(record: Record<CborValue>) -> Self {
        Self::now(record, RowStatus::PendingWrite)
    }

    /// Wrap a row whose optimistic write failed and was rolled back: `record`
    /// is the restored pre-image, `status` carries the error for the UI.
    pub fn write_failed(record: Record<CborValue>, error: String) -> Self {
        Self::now(record, RowStatus::WriteFailed { error })
    }

    /// Mark a row whose detail pass failed. Keeps the partial (list-pass)
    /// `record` so the row stays visible, but records the error so the UI can
    /// surface it. Carries over the previous `fetched_at`.
    pub fn detail_failed(
        record: Record<CborValue>,
        error: String,
        fetched_at: Option<SystemTime>,
    ) -> Self {
        Self {
            record,
            status: RowStatus::LoadFailed { error },
            fetched_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowStatus {
    Fresh,
    /// Only the list pass has run — cheap columns present, detail columns
    /// awaiting hydration by the detail pass.
    Incomplete,
    PendingWrite,
    WriteFailed {
        error: String,
    },
    /// The two-pass detail fetch for this row failed; the partial list-pass
    /// columns remain visible. Distinct from [`WriteFailed`](RowStatus::WriteFailed),
    /// which is a write-back error.
    LoadFailed {
        error: String,
    },
}
