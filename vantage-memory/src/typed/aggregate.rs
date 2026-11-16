//! Sum, max and min over the rows a table's conditions select (window
//! ignored), and the aggregate a roll-up computes over its child cells.

use std::cmp::Ordering;
use std::str::FromStr;

use ciborium::Value as CborValue;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use vantage_core::Result;
use vantage_table::column::core::Column;
use vantage_table::table::{RollupOp, Table};
use vantage_types::Entity;

use super::{Fill, MemoryDB};
use crate::eval::compare::cmp_values;
use crate::types::AnyMemoryType;

/// The CBOR tag of a decimal carried as text.
const DECIMAL_TAG: u64 = 10;

impl MemoryDB {
    /// Summed as [`sum`] does; an empty set sums to `0`.
    pub(super) async fn sum<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        column: &Column<AnyMemoryType>,
    ) -> Result<AnyMemoryType> {
        Ok(AnyMemoryType::untyped(sum(&self
            .column_cells(table, column)
            .await?)))
    }

    /// The greatest (`Greater`) or least (`Less`) non-null cell; `Null` when none.
    pub(super) async fn extreme<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        column: &Column<AnyMemoryType>,
        keep: Ordering,
    ) -> Result<AnyMemoryType> {
        let cells = self.column_cells(table, column).await?;
        Ok(AnyMemoryType::untyped(best_of(cells, keep)))
    }

    async fn column_cells<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        column: &Column<AnyMemoryType>,
    ) -> Result<Vec<CborValue>> {
        let rows = self.rows(table, false, Fill::Column(column.name())).await?;
        Ok(rows
            .iter()
            .filter_map(|(_, row)| row.get(column.name()).cloned())
            .filter(|v| !matches!(v, CborValue::Null))
            .collect())
    }
}

/// `op` over a roll-up's child cells. Over no rows `Count` and `Sum` are 0,
/// the others null.
pub(super) fn rollup_value(op: RollupOp, cells: &[CborValue]) -> CborValue {
    let present = || cells.iter().filter(|c| !c.is_null()).cloned().collect();
    match op {
        RollupOp::Count => CborValue::Integer((cells.len() as u64).into()),
        RollupOp::Sum => sum(cells),
        RollupOp::Min => best_of(present(), Ordering::Less),
        RollupOp::Max => best_of(present(), Ordering::Greater),
        RollupOp::Avg => average(cells),
    }
}

/// A float makes the average a float; otherwise a decimal makes it a
/// decimal. Integers alone average to a float.
fn average(cells: &[CborValue]) -> CborValue {
    let numbers: Vec<Number> = cells.iter().filter_map(Number::of).collect();
    if numbers.is_empty() {
        return CborValue::Null;
    }
    let n = numbers.len();
    match total(&numbers) {
        Number::Decimal(d) => decimal_cell(d / Decimal::from(n as u64)),
        other => CborValue::Float(other.as_f64() / n as f64),
    }
}

/// The greatest (`Greater`) or least (`Less`) of `cells`; `Null` when empty.
/// Numbers compare across integer, float and decimal.
fn best_of(cells: Vec<CborValue>, keep: Ordering) -> CborValue {
    cells
        .into_iter()
        .reduce(|best, v| {
            let order = match (Number::of(&v), Number::of(&best)) {
                (Some(a), Some(b)) => a.cmp(&b),
                _ => cmp_values(&v, &best),
            };
            match order {
                Some(o) if o == keep => v,
                _ => best,
            }
        })
        .unwrap_or(CborValue::Null)
}

/// Integers stay integer; a decimal makes the sum a decimal and a float a
/// float, as does a decimal sum past `Decimal`'s range. Non-numeric cells
/// are skipped.
fn sum(cells: &[CborValue]) -> CborValue {
    let numbers: Vec<Number> = cells.iter().filter_map(Number::of).collect();
    match total(&numbers) {
        Number::Int(int) => ciborium::value::Integer::try_from(int)
            .map(CborValue::Integer)
            .unwrap_or(CborValue::Float(int as f64)),
        Number::Decimal(d) => decimal_cell(d),
        Number::Float(f) => CborValue::Float(f),
    }
}

/// A numeric cell: an integer, a float, or a decimal (`Tag(10)` text).
#[derive(Clone, Copy)]
enum Number {
    Int(i128),
    Decimal(Decimal),
    Float(f64),
}

impl Number {
    fn of(cell: &CborValue) -> Option<Self> {
        match cell {
            CborValue::Integer(i) => Some(Number::Int(i128::from(*i))),
            CborValue::Float(f) => Some(Number::Float(*f)),
            CborValue::Tag(DECIMAL_TAG, inner) => match inner.as_ref() {
                CborValue::Text(s) => Decimal::from_str(s)
                    .or_else(|_| Decimal::from_scientific(s))
                    .ok()
                    .map(Number::Decimal),
                _ => None,
            },
            _ => None,
        }
    }

    fn as_f64(self) -> f64 {
        match self {
            Number::Int(i) => i as f64,
            Number::Decimal(d) => d.to_f64().unwrap_or(f64::NAN),
            Number::Float(f) => f,
        }
    }

    fn as_decimal(self) -> Option<Decimal> {
        match self {
            Number::Int(i) => Decimal::try_from_i128_with_scale(i, 0).ok(),
            Number::Decimal(d) => Some(d),
            Number::Float(_) => None,
        }
    }

    /// Exact between integers and decimals; through `f64` with a float.
    fn cmp(&self, other: &Number) -> Option<Ordering> {
        match (self, other) {
            (Number::Int(a), Number::Int(b)) => Some(a.cmp(b)),
            (Number::Float(_), _) | (_, Number::Float(_)) => {
                self.as_f64().partial_cmp(&other.as_f64())
            }
            _ => Some(self.as_decimal()?.cmp(&other.as_decimal()?)),
        }
    }
}

/// The sum of `numbers` in the widest kind present: float, then decimal,
/// then integer. A decimal sum that overflows is a float. An empty list is
/// integer `0`.
fn total(numbers: &[Number]) -> Number {
    let float = || Number::Float(numbers.iter().map(|n| n.as_f64()).sum());
    if numbers.iter().any(|n| matches!(n, Number::Float(_))) {
        return float();
    }
    if numbers.iter().any(|n| matches!(n, Number::Decimal(_))) {
        let exact = numbers
            .iter()
            .try_fold(Decimal::ZERO, |sum, n| sum.checked_add(n.as_decimal()?));
        return exact.map_or_else(float, Number::Decimal);
    }
    Number::Int(
        numbers
            .iter()
            .map(|n| match n {
                Number::Int(i) => *i,
                _ => 0,
            })
            .sum(),
    )
}

fn decimal_cell(d: Decimal) -> CborValue {
    CborValue::Tag(DECIMAL_TAG, Box::new(CborValue::Text(d.to_string())))
}
