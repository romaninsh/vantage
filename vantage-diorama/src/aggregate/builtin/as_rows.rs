//! Presenting a scalar reduction as the one-row set it actually is.
//!
//! `count(*)` in SQL yields a table with one row and one column, not a bare
//! number, and everything downstream is simpler when we agree with SQL: a
//! scalar aggregate mounts as an ordinary table, so a grid, a chart and a
//! single-figure tile all observe it through the one contract they already
//! have. Without this, a scalar needs a parallel "value" path at every layer —
//! its own scenery type, its own observation, and a special case in every
//! component that might display it.

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{Column, ColumnFlag, VistaMetadata};

use crate::aggregate::aggregation::{Aggregation, DerivedRows, Reads, Rows};

/// The id of a scalar aggregate's only row. Stable, so a recomputation
/// updates that row rather than replacing one row with another.
const ROW_ID: &str = "value";

/// Wraps a scalar aggregation so it produces a one-row [`DerivedRows`] whose
/// single column is named by `alias`.
pub struct AsRows<A> {
    alias: String,
    inner: A,
}

impl<A> AsRows<A> {
    pub fn new(alias: impl Into<String>, inner: A) -> Self {
        Self {
            alias: alias.into(),
            inner,
        }
    }
}

impl<A: Aggregation<Output = CborValue>> Aggregation for AsRows<A> {
    type Output = DerivedRows;

    fn reads(&self) -> Reads {
        self.inner.reads()
    }

    fn compute(&self, rows: &Rows, schema: &VistaMetadata) -> DerivedRows {
        let value = self.inner.compute(rows, schema);
        let column = Column::new(self.alias.clone(), column_type(&value)).with_flag(ColumnFlag::Id);
        let mut record = Record::new();
        record.insert(self.alias.clone(), value);
        DerivedRows::new(vec![(ROW_ID.to_string(), record)], vec![column])
    }
}

/// The declared type of the single column, read from the value.
///
/// A reduction whose result changes kind — a `sum` that was integral until a
/// fractional row arrived — reports a schema change, which is honest: the
/// column really did change type. Reductions in practice stay in one kind.
fn column_type(value: &CborValue) -> &'static str {
    match value {
        CborValue::Integer(_) => "i64",
        CborValue::Float(_) => "f64",
        CborValue::Bool(_) => "bool",
        _ => "String",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::builtin::fixtures::{events, schema, text};
    use crate::aggregate::builtin::{Conditions, Count, Sum, Where};

    /// The contract: one row, one column, named by the alias.
    #[test]
    fn a_scalar_becomes_a_one_row_one_column_table() {
        let derived = AsRows::new("total", Count::rows()).compute(&events(), &schema());
        assert_eq!(derived.len(), 1);
        assert_eq!(derived.columns.len(), 1);
        assert_eq!(derived.columns[0].name, "total");
        let (_, row) = &derived.rows[0];
        assert_eq!(row.get("total"), Some(&CborValue::Integer(3.into())));
    }

    /// Diorama addresses rows by id, so the derived table needs one — and the
    /// single column has to carry it, there being no other.
    #[test]
    fn the_only_column_is_the_id_column() {
        let derived = AsRows::new("total", Count::rows()).compute(&events(), &schema());
        assert!(
            derived.columns[0].has_flag(ColumnFlag::Id),
            "columns: {:?}",
            derived.columns
        );
    }

    /// A stable row id means a recomputation UPDATES the row rather than
    /// deleting one and inserting another — which a subscribed grid would
    /// otherwise see as the row vanishing and reappearing.
    #[test]
    fn the_row_id_is_stable_across_recomputations() {
        let agg = AsRows::new("total", Count::rows());
        let first = agg.compute(&events(), &schema());
        let second = agg.compute(&Rows::new(), &schema());
        assert_eq!(first.rows[0].0, second.rows[0].0);
        assert_eq!(
            second.rows[0].1.get("total"),
            Some(&CborValue::Integer(0.into())),
            "an empty source still yields one row, holding zero",
        );
    }

    /// Composition is the point: narrowing and reduction stay separate, and
    /// this only changes the shape of the answer.
    #[test]
    fn it_wraps_a_narrowed_reduction_unchanged() {
        let agg = AsRows::new(
            "opened_bytes",
            Where::new(
                Conditions::new().eq("Event", text("opened")),
                Sum::of("Size"),
            ),
        );
        let derived = agg.compute(&events(), &schema());
        assert_eq!(
            derived.rows[0].1.get("opened_bytes"),
            Some(&CborValue::Integer(40.into())),
        );
        assert_eq!(derived.columns[0].name, "opened_bytes");
    }

    #[test]
    fn the_column_type_follows_the_value() {
        let ints = AsRows::new("n", Count::rows()).compute(&events(), &schema());
        assert_eq!(ints.columns[0].original_type.as_deref(), Some("i64"));
    }
}
