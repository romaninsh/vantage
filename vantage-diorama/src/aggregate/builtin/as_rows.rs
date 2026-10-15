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
