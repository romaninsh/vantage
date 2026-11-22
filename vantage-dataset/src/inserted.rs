use vantage_types::Record;

/// What an insert did with the id it was given.
#[derive(Debug, Clone, PartialEq)]
pub enum Inserted<V> {
    /// Stored now and confirmed; the record as stored.
    Created(Record<V>),
    /// The id was already in the set; nothing written; the stored record.
    Existed(Record<V>),
    /// Handed to a backend that confirms later or never (queue, webhook,
    /// batched API). The caller may assume it will be stored; it may yet be
    /// rejected or turn out to be a duplicate.
    Accepted,
}

impl<V> Inserted<V> {
    /// The stored record, unless the backend only accepted the write.
    pub fn record(&self) -> Option<&Record<V>> {
        match self {
            Inserted::Created(r) | Inserted::Existed(r) => Some(r),
            Inserted::Accepted => None,
        }
    }

    pub fn into_record(self) -> Option<Record<V>> {
        match self {
            Inserted::Created(r) | Inserted::Existed(r) => Some(r),
            Inserted::Accepted => None,
        }
    }

    /// True when the backend confirmed the row is stored (`Created` or `Existed`).
    pub fn is_confirmed(&self) -> bool {
        !matches!(self, Inserted::Accepted)
    }
}
