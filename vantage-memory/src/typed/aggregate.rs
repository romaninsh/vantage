//! Sum, max and min over the rows a table's conditions select (window ignored).

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_types::Entity;

use super::MemoryDB;
use crate::eval::compare::cmp_values;
use crate::types::AnyMemoryType;

impl MemoryDB {
    /// Integers stay integer until a float appears; non-numeric cells are
    /// skipped; an empty set sums to `0`.
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
        let best = self
            .column_cells(table, column)
            .await?
            .into_iter()
            .reduce(|best, v| match cmp_values(&v, &best) {
                Some(o) if o == keep => v,
                _ => best,
            });
        Ok(AnyMemoryType::untyped(best.unwrap_or(CborValue::Null)))
    }

    async fn column_cells<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        column: &Column<AnyMemoryType>,
    ) -> Result<Vec<CborValue>> {
        let rows = self.rows(table, false).await?;
        Ok(rows
            .iter()
            .filter_map(|(_, row)| row.get(column.name()).cloned())
            .filter(|v| !matches!(v, CborValue::Null))
            .collect())
    }
}

fn sum(cells: &[CborValue]) -> CborValue {
    let mut int: i128 = 0;
    let mut float: Option<f64> = None;
    for cell in cells {
        match cell {
            CborValue::Integer(i) => int += i128::from(*i),
            CborValue::Float(f) => *float.get_or_insert(0.0) += f,
            _ => {}
        }
    }
    match float {
        Some(f) => CborValue::Float(f + int as f64),
        None => ciborium::value::Integer::try_from(int)
            .map(CborValue::Integer)
            .unwrap_or(CborValue::Float(int as f64)),
    }
}
