//! Counting.

use ciborium::Value as CborValue;

use crate::aggregation::{Aggregation, Rows};
use crate::builtin::filter::{Conditions, Where};

/// Number of rows.
///
/// Counts what the source currently holds. On a lazily-paged source that is the
/// rows loaded so far, climbing as more arrive — the same coverage every
/// client-side aggregate has, and the reason a total shown beside a partially
/// loaded grid should be labelled as observed rather than final.
pub struct Count;

impl Count {
    pub fn rows() -> Self {
        Self
    }
}

impl Aggregation for Count {
    type Output = CborValue;

    fn compute(&self, rows: &Rows) -> CborValue {
        CborValue::Integer((rows.len() as i64).into())
    }
}

/// Number of rows where every named column equals the given value.
pub struct CountWhere {
    conditions: Vec<(String, CborValue)>,
}

impl CountWhere {
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    pub fn eq(mut self, column: impl Into<String>, value: impl Into<CborValue>) -> Self {
        self.conditions.push((column.into(), value.into()));
        self
    }
}

impl Default for CountWhere {
    fn default() -> Self {
        Self::new()
    }
}

impl Aggregation for CountWhere {
    type Output = CborValue;

    /// Delegates to [`Where`]`<`[`Count`]`>` — the narrowing lives with the
    /// other combinators so a filtered count and a filtered sum can never
    /// disagree about what a term means.
    fn compute(&self, rows: &Rows) -> CborValue {
        let mut conditions = Conditions::new();
        for (column, expected) in &self.conditions {
            conditions = conditions.eq(column.clone(), expected.clone());
        }
        Where::new(conditions, Count::rows()).compute(rows)
    }
}
