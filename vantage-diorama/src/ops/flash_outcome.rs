//! [`FlashOutcome`]: what a confirmed write reports back.

use ciborium::Value as CborValue;
use vantage_types::Record;

/// The result of a write a [`Lens`](crate::Lens) writer (or the master)
/// confirmed.
///
/// `id` is the id the row settled under when the backend chose one; `record`
/// is the row as the backend stored it, when it reported one. Both are
/// optional: a write that only says "done" is [`applied`](Self::applied).
/// `confirmation` says what an insert did with its id.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FlashOutcome {
    pub id: Option<String>,
    pub record: Option<Record<CborValue>>,
    pub confirmation: Confirmation,
}

/// How the backend confirmed a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Confirmation {
    /// Any write other than an insert: it landed.
    #[default]
    Applied,
    /// The insert stored a new row; `record` is the row as stored.
    Created,
    /// The id was already there and nothing was written; `record` is the
    /// row the backend holds, which the cache now shows.
    Existed,
    /// The backend took the insert to store later, or never. The row stays
    /// in this Dio's cache unconfirmed until a read of the master finds it.
    Accepted,
}

impl FlashOutcome {
    /// The write landed and the backend reported nothing further.
    pub fn applied() -> Self {
        Self {
            id: None,
            record: None,
            confirmation: Confirmation::Applied,
        }
    }

    pub fn with_confirmation(mut self, confirmation: Confirmation) -> Self {
        self.confirmation = confirmation;
        self
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
