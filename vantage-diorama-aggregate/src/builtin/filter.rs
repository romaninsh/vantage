//! Narrowing the rows an aggregation sees.
//!
//! Every reduction in this crate answers over "the rows it was given", so
//! restricting *which* rows is a separate, orthogonal concern — not something
//! each reduction should re-implement. [`Where`] wraps any
//! [`crate::Aggregation`] and hands it a narrowed set, so `sum of
//! Size where Event = opened` composes out of the two pieces already here
//! instead of needing a `SumWhere`.

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::eval::cell_get;
use vantage_vista::{Column, Condition, FilterOp, VistaMetadata};

use crate::aggregation::{Aggregation, Rows};
use crate::cmp;

#[cfg(test)]
mod tests;

/// Equality terms, ANDed together.
///
/// # Matching
///
/// A term is a vista [`Condition`], matched by vista's in-memory evaluator,
/// so an integer and a float of the same number are equal (`3 == 3.0`). When
/// that fails, the row's value and the expected value are compared as
/// rendered scalars. The second pass is what makes configuration-driven
/// conditions usable: a YAML file says `code: 200` with no way to know
/// whether the backend sends `200` or `"200"`, and a filter that silently
/// matched nothing because the column was text would be a wrong answer
/// rather than an error.
///
/// A term naming a column the row doesn't have never matches, and neither
/// does a null cell against a number. Absence is not a value, so treating a
/// missing column as a pass would let a typo silently widen the aggregation
/// to every row.
#[derive(Debug, Clone, Default)]
pub struct Conditions {
    terms: Vec<Condition>,
    /// Declares every term's column as a flat key. The evaluator reads a
    /// dotted column as a nested path unless the schema declares it, and an
    /// aggregation never sees its source's schema, so a row that holds the
    /// column's literal key is read with this.
    flat: VistaMetadata,
}

impl Conditions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `column == value`. The column may be a dotted path into a nested
    /// record (`Message.Headers.Subject`); a row holding the literal dotted
    /// key is read by that key instead.
    pub fn eq(mut self, column: impl Into<String>, value: impl Into<CborValue>) -> Self {
        let column = column.into();
        self.flat
            .columns
            .insert(column.clone(), Column::new(column.clone(), "any"));
        self.terms.push(Condition::new(column, FilterOp::Eq, value));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Whether one row satisfies every term.
    pub fn matches(&self, record: &Record<CborValue>) -> bool {
        let nested = VistaMetadata::new();
        self.terms.iter().all(|term| {
            let metadata = if record.contains_key(&term.column) {
                &self.flat
            } else {
                &nested
            };
            term.matches(record, metadata)
                || cell_get(record, &term.column, metadata)
                    .is_some_and(|actual| same_scalar_text(actual, &term.value))
        })
    }

    /// Narrow a row set to the rows that match. Returns the input untouched
    /// when there are no terms, so an unconditioned aggregation pays nothing.
    pub fn narrow<'a>(&self, rows: &'a Rows) -> std::borrow::Cow<'a, Rows> {
        if self.is_empty() {
            return std::borrow::Cow::Borrowed(rows);
        }
        std::borrow::Cow::Owned(
            rows.iter()
                .filter(|(_, record)| self.matches(record))
                .map(|(id, record)| (id.clone(), record.clone()))
                .collect(),
        )
    }
}

/// Whether two scalars render to the same text, so a configured `"200"`
/// matches an integer `200`. See [`Conditions`].
fn same_scalar_text(actual: &CborValue, expected: &CborValue) -> bool {
    match (cmp::scalar_text(actual), cmp::scalar_text(expected)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
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

    fn compute(&self, rows: &Rows) -> Self::Output {
        self.inner.compute(&self.conditions.narrow(rows))
    }
}
