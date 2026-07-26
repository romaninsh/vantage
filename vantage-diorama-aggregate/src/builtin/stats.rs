//! Numeric reductions over one column.
//!
//! Each is a plain fold over a snapshot — the payoff of recomputing rather than
//! maintaining state. `Max` and `Min` in particular need no special handling
//! for a removed extreme, which is the case incremental aggregation cannot
//! express without either an invertible fold or a fallback rebuild.

use ciborium::Value as CborValue;

use crate::aggregation::{Aggregation, Rows};
use crate::cmp;

/// Numeric cells of `column`, skipping rows where it is absent or non-numeric.
fn numbers<'a>(rows: &'a Rows, column: &'a str) -> impl Iterator<Item = f64> + 'a {
    rows.values()
        .filter_map(move |record| cmp::column(record, column))
        .filter_map(cmp::as_f64)
}

/// Represent a float as an integer when it is exactly one, so a sum over
/// integer columns reads as an integer rather than `42.0`.
fn number(value: f64) -> CborValue {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
        CborValue::Integer((value as i64).into())
    } else {
        CborValue::Float(value)
    }
}

macro_rules! column_aggregation {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        pub struct $name {
            column: String,
        }

        impl $name {
            pub fn new(column: impl Into<String>) -> Self {
                Self {
                    column: column.into(),
                }
            }
        }
    };
}

column_aggregation!(Sum, "Sum of a numeric column. Empty input sums to zero.");
column_aggregation!(
    Avg,
    "Mean of a numeric column. Null when no row carries a number."
);
column_aggregation!(
    Max,
    "Largest value of a numeric column. Null when no row carries a number."
);
column_aggregation!(
    Min,
    "Smallest value of a numeric column. Null when no row carries a number."
);

impl Aggregation for Sum {
    type Output = CborValue;

    fn compute(&self, rows: &Rows) -> CborValue {
        number(numbers(rows, &self.column).sum::<f64>())
    }
}

impl Aggregation for Avg {
    type Output = CborValue;

    fn compute(&self, rows: &Rows) -> CborValue {
        let mut count = 0usize;
        let mut total = 0.0;
        for value in numbers(rows, &self.column) {
            total += value;
            count += 1;
        }
        if count == 0 {
            return CborValue::Null;
        }
        CborValue::Float(total / count as f64)
    }
}

impl Aggregation for Max {
    type Output = CborValue;

    fn compute(&self, rows: &Rows) -> CborValue {
        numbers(rows, &self.column).fold(CborValue::Null, |acc, value| match &acc {
            CborValue::Null => number(value),
            existing => match cmp::as_f64(existing) {
                Some(current) if value > current => number(value),
                _ => acc,
            },
        })
    }
}

impl Aggregation for Min {
    type Output = CborValue;

    fn compute(&self, rows: &Rows) -> CborValue {
        numbers(rows, &self.column).fold(CborValue::Null, |acc, value| match &acc {
            CborValue::Null => number(value),
            existing => match cmp::as_f64(existing) {
                Some(current) if value < current => number(value),
                _ => acc,
            },
        })
    }
}

/// Number of distinct values in a column, compared by their CBOR representation.
pub struct Distinct {
    column: String,
}

impl Distinct {
    pub fn new(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
        }
    }
}

impl Aggregation for Distinct {
    type Output = CborValue;

    fn compute(&self, rows: &Rows) -> CborValue {
        let mut seen: Vec<&CborValue> = Vec::new();
        for record in rows.values() {
            if let Some(value) = cmp::column(record, &self.column)
                && !seen.contains(&value)
            {
                seen.push(value);
            }
        }
        CborValue::Integer((seen.len() as i64).into())
    }
}
