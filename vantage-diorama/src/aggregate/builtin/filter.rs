//! Narrowing the rows an aggregation sees.
//!
//! Every reduction in this module answers over "the rows it was given", so
//! restricting *which* rows is a separate, orthogonal concern — not something
//! each reduction should re-implement. [`Where`] wraps any
//! [`crate::aggregate::Aggregation`] and hands it a narrowed set, so `sum of
//! Size where Event = opened` composes out of the two pieces already here
//! instead of needing a `SumWhere`.

use std::borrow::Cow;

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{Condition, FilterOp, VistaMetadata};

use crate::aggregate::aggregation::{Aggregation, Reads, Rows};
use crate::aggregate::coerce::coerce_operand;

#[cfg(test)]
mod tests;

/// Equality terms, ANDed together.
///
/// # Matching
///
/// A term is a vista [`Condition`], matched by vista's in-memory evaluator
/// against the source schema: a dotted column the schema declares is a flat
/// key, any other dotted column a path into nested maps, and an integer and a
/// float of the same number are equal (`3 == 3.0`).
///
/// Before matching, each operand is converted to the type its column
/// declares. Configuration can't know the backend's types: a YAML file says
/// `code: 200` whether the column holds `200` or `"200"`, and a filter that
/// silently matched nothing would be a wrong answer rather than an error. A
/// column with no declared type compares the operand as given, so `"200"`
/// doesn't match `200` there. Terms are equality only; an ordered term added
/// later must be coerced the same way, or `"9" < 10` would compare text.
///
/// A term naming a column the row doesn't have never matches, and neither
/// does a null cell against a number. Absence is not a value, so treating a
/// missing column as a pass would let a typo silently widen the aggregation
/// to every row.
#[derive(Debug, Clone, Default)]
pub struct Conditions {
    terms: Vec<Condition>,
}

impl Conditions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `column == value`. The column may be a dotted path into a nested
    /// record (`Message.Headers.Subject`).
    pub fn eq(mut self, column: impl Into<String>, value: impl Into<CborValue>) -> Self {
        self.terms.push(Condition::new(column.into(), FilterOp::Eq, value));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// The columns the terms name.
    pub(crate) fn reads(&self) -> Reads {
        let columns = self.terms.iter().map(|t| t.column.clone()).collect();
        Reads::Columns(Vec::new()).and(Reads::Columns(columns))
    }

    /// The terms with each operand converted to its column's declared type.
    fn coerced(&self, schema: &VistaMetadata) -> Vec<Condition> {
        self.terms
            .iter()
            .map(|term| {
                let mut term = term.clone();
                term.value = coerce_operand(&term.value, schema.columns.get(&term.column));
                term
            })
            .collect()
    }

    /// Whether one row of a source described by `schema` satisfies every
    /// term.
    pub fn matches(&self, record: &Record<CborValue>, schema: &VistaMetadata) -> bool {
        all_match(&self.coerced(schema), record, schema)
    }

    /// Narrow a row set to the rows that match. Returns the input untouched
    /// when there are no terms, so an unconditioned aggregation pays nothing.
    pub fn narrow<'a>(&self, rows: &'a Rows, schema: &VistaMetadata) -> Cow<'a, Rows> {
        if self.is_empty() {
            return Cow::Borrowed(rows);
        }
        let terms = self.coerced(schema);
        Cow::Owned(
            rows.iter()
                .filter(|(_, record)| all_match(&terms, record, schema))
                .map(|(id, record)| (id.clone(), record.clone()))
                .collect(),
        )
    }
}

fn all_match(terms: &[Condition], record: &Record<CborValue>, schema: &VistaMetadata) -> bool {
    terms.iter().all(|term| term.matches(record, schema))
}

/// Run `inner` over only the rows matching `conditions`.
///
/// Works with any aggregation, scalar or row-producing — a filtered
/// `GroupBy` is `Where::new(conditions, GroupBy::column(..))` and needs no
/// special support from either side.
pub struct Where<A> {
    conditions: Conditions,
    inner: A,
}

impl<A> Where<A> {
    pub fn new(conditions: Conditions, inner: A) -> Self {
        Self { conditions, inner }
    }
}

impl<A: Aggregation> Aggregation for Where<A> {
    type Output = A::Output;

    fn reads(&self) -> Reads {
        self.conditions.reads().and(self.inner.reads())
    }

    fn compute(&self, rows: &Rows, schema: &VistaMetadata) -> Self::Output {
        self.inner.compute(&self.conditions.narrow(rows, schema), schema)
    }
}
