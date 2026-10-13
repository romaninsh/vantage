//! [`FlashOutcome`]: what a confirmed write reports back.

use ciborium::Value as CborValue;
use vantage_types::Record;

/// The result of a write a [`Lens`](crate::Lens) writer (or the master)
/// confirmed.
///
/// `id` is the id the row settled under when the backend chose one; `record`
/// is the row as the backend stored it, when it reported one. Both are
/// optional: a write that only says "done" is [`applied`](Self::applied).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FlashOutcome {
    pub id: Option<String>,
    pub record: Option<Record<CborValue>>,
}

impl FlashOutcome {
    /// The write landed and the backend reported nothing further.
    pub fn applied() -> Self {
        Self {
            id: None,
            record: None,
        }
    }

    /// The id the row settled under.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The row as the backend stored it.
    pub fn with_record(mut self, record: Record<CborValue>) -> Self {
        self.record = Some(record);
        self
    }
}
