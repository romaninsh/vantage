//! Change notifications a table broadcasts after each write that changed a row.

use crate::store::Row;

/// Channel backlog before a lagging subscriber gets `Lagged` and must re-list.
pub(crate) const EVENT_CAPACITY: usize = 4096;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum MemoryChange {
    Inserted {
        id: String,
        row: Row,
    },
    Updated {
        id: String,
        row: Row,
        old: Row,
    },
    Deleted {
        id: String,
        old: Row,
    },
    /// Rows changed while the table was quiet; subscribers must re-list.
    Reset,
}

impl MemoryChange {
    /// The row the change is about; `None` for `Reset`.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Inserted { id, .. } | Self::Updated { id, .. } | Self::Deleted { id, .. } => {
                Some(id)
            }
            Self::Reset => None,
        }
    }
}
