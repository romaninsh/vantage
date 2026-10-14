//! Counting.

use ciborium::Value as CborValue;
use vantage_vista::VistaMetadata;

use crate::aggregate::aggregation::{Aggregation, Reads, Rows};

/// Number of rows.
///
/// Counts what the source currently holds. On a lazily-paged source that is the
/// rows loaded so far, climbing as more arrive — the same coverage every
/// client-side aggregate has, and the reason a total shown beside a partially
/// loaded grid should be labelled as observed rather than final.
///
/// A filtered count is [`Where`](crate::aggregate::Where) over this.
pub struct Count;

impl Count {
    pub fn rows() -> Self {
        Self
    }
}

impl Aggregation for Count {
    type Output = CborValue;

    fn reads(&self) -> Reads {
        Reads::Count
    }

    fn compute(&self, rows: &Rows, _schema: &VistaMetadata) -> CborValue {
        CborValue::Integer((rows.len() as i64).into())
    }
}
