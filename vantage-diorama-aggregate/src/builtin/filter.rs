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

use crate::aggregation::{Aggregation, Rows};
use crate::cmp;

/// Equality terms, ANDed together.
///
/// # Matching
///
/// A term matches when the row's value equals the expected value, compared
/// first as CBOR and then — only if that fails — as rendered scalars. The
/// second pass is what makes configuration-driven conditions usable: a YAML
/// file says `code: 200` with no way to know whether the backend sends
/// `200` or `"200"`, and a filter that silently matched nothing because the
/// column was text would be a wrong answer rather than an error.
///
/// A term naming a column the row doesn't have never matches. Absence is not
/// a value, so treating a missing column as a pass would let a typo silently
/// widen the aggregation to every row.
#[derive(Debug, Clone, Default)]
pub struct Conditions {
    terms: Vec<(String, CborValue)>,
}

impl Conditions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `column == value`. The column may be a dotted path into a nested
    /// record (`Message.Headers.Subject`).
    pub fn eq(mut self, column: impl Into<String>, value: impl Into<CborValue>) -> Self {
        self.terms.push((column.into(), value.into()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Whether one row satisfies every term.
    pub fn matches(&self, record: &Record<CborValue>) -> bool {
        self.terms.iter().all(|(column, expected)| {
            cmp::column(record, column).is_some_and(|actual| value_eq(actual, expected))
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

/// CBOR equality, falling back to comparing scalar renderings so a
/// configured `"200"` matches an integer `200`. See [`Conditions`].
fn value_eq(actual: &CborValue, expected: &CborValue) -> bool {
    if actual == expected {
        return true;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{Count, Sum};

    fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
        let mut r = Record::new();
        for (k, v) in pairs {
            r.insert((*k).to_string(), v.clone());
        }
        r
    }

    fn rows(records: Vec<Record<CborValue>>) -> Rows {
        records
            .into_iter()
            .enumerate()
            .map(|(i, r)| (i.to_string(), r))
            .collect()
    }

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.to_string())
    }

    fn events() -> Rows {
        rows(vec![
            record(&[
                ("Event", text("opened")),
                ("Size", CborValue::Integer(10.into())),
            ]),
            record(&[
                ("Event", text("failed")),
                ("Size", CborValue::Integer(20.into())),
            ]),
            record(&[
                ("Event", text("opened")),
                ("Size", CborValue::Integer(30.into())),
            ]),
        ])
    }

    #[test]
    fn where_narrows_a_count() {
        let agg = Where::new(Conditions::new().eq("Event", text("opened")), Count::rows());
        assert_eq!(agg.compute(&events()), CborValue::Integer(2.into()));
    }

    /// The reason this is a combinator and not a `CountWhere` twin: the same
    /// conditions narrow any reduction.
    #[test]
    fn where_narrows_a_sum() {
        let agg = Where::new(
            Conditions::new().eq("Event", text("opened")),
            Sum::new("Size"),
        );
        // 10 + 30, not 60.
        assert_eq!(agg.compute(&events()), CborValue::Integer(40.into()));
    }

    #[test]
    fn no_conditions_passes_every_row_through() {
        let agg = Where::new(Conditions::new(), Count::rows());
        assert_eq!(agg.compute(&events()), CborValue::Integer(3.into()));
    }

    #[test]
    fn a_term_on_an_absent_column_matches_nothing() {
        let agg = Where::new(Conditions::new().eq("Nope", text("opened")), Count::rows());
        assert_eq!(agg.compute(&events()), CborValue::Integer(0.into()));
    }

    /// Configuration carries strings; the data may not. Matching only on CBOR
    /// equality would make `code: 200` in a YAML file silently count zero.
    #[test]
    fn a_text_term_matches_a_numeric_column() {
        let numeric = rows(vec![
            record(&[("code", CborValue::Integer(200.into()))]),
            record(&[("code", CborValue::Integer(500.into()))]),
        ]);
        let agg = Where::new(Conditions::new().eq("code", text("200")), Count::rows());
        assert_eq!(agg.compute(&numeric), CborValue::Integer(1.into()));
    }

    #[test]
    fn terms_reach_into_nested_records_by_dotted_path() {
        let nested = rows(vec![
            record(&[(
                "Message",
                CborValue::Map(vec![(text("Transport"), text("smtp"))]),
            )]),
            record(&[(
                "Message",
                CborValue::Map(vec![(text("Transport"), text("api"))]),
            )]),
        ]);
        let agg = Where::new(
            Conditions::new().eq("Message.Transport", text("smtp")),
            Count::rows(),
        );
        assert_eq!(agg.compute(&nested), CborValue::Integer(1.into()));
    }

    #[test]
    fn terms_are_anded() {
        let agg = Where::new(
            Conditions::new()
                .eq("Event", text("opened"))
                .eq("Size", CborValue::Integer(30.into())),
            Count::rows(),
        );
        assert_eq!(agg.compute(&events()), CborValue::Integer(1.into()));
    }
}
