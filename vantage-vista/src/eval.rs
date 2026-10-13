//! Evaluating a [`VistaQuery`] over rows held in memory.
//!
//! Every caller that applies clauses to rows itself (a shell over a store it
//! holds, a cache answering what its master refused, a client-side refine)
//! uses this one evaluator, so a clause means the same thing wherever it is
//! evaluated:
//!
//! - a cell is read by [`cell_get`], for conditions and order alike: a flat
//!   key, or a dotted path into nested maps;
//! - values compare by [`cbor_cmp`]: integers and floats numerically, and
//!   `3 == 3.0` for equality and set membership;
//! - a sort puts absent and null cells last in either direction, and breaks
//!   ties on id;
//! - search is a case-insensitive substring of any cell's text: text,
//!   numbers, bools, tagged values such as record ids, and the values
//!   nested in arrays and maps. An empty needle matches every row.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::{
    filter::{FilterOp, operand_text},
    metadata::VistaMetadata,
    query::{Condition, VistaQuery},
    sort::SortDirection,
};

mod compare;
mod path;
mod search;
#[cfg(test)]
mod tests;

use search::matches_search;

pub use compare::cbor_cmp;
pub use path::{cell_get, path_get};

type Rec = Record<CborValue>;

impl Condition {
    /// Whether `row` satisfies this condition, its cell read by
    /// [`cell_get`]. Equality and set membership take an integer and a float
    /// of the same number as equal (`3 == 3.0`); the ordered operators
    /// compare by [`cbor_cmp`]. An absent cell matches only `Ne` and
    /// `NotInSet`.
    pub fn matches(&self, row: &Rec, metadata: &VistaMetadata) -> bool {
        let cell = cell_get(row, &self.column, metadata);
        let operand = &self.value;
        match self.op {
            FilterOp::Eq => cell.is_some_and(|c| same(c, operand)),
            FilterOp::Ne => !cell.is_some_and(|c| same(c, operand)),
            FilterOp::InSet => in_set(cell, operand),
            FilterOp::NotInSet => !in_set(cell, operand),
            FilterOp::Like => cell.is_some_and(|c| {
                operand_text(c)
                    .to_lowercase()
                    .contains(&operand_text(operand).to_lowercase())
            }),
            op => cell.is_some_and(|c| op.matches_ordering(cbor_cmp(c, operand))),
        }
    }
}

impl VistaQuery {
    /// Whether `row` satisfies every condition and the search. The order
    /// doesn't narrow, so it is ignored.
    ///
    /// A native condition is opaque and can't be evaluated in memory: a
    /// query holding any is an `IncorrectUsage` error. Check
    /// [`is_locally_evaluable`](Self::is_locally_evaluable) first.
    pub fn matches(&self, row: &Rec, metadata: &VistaMetadata) -> Result<bool> {
        if !self.is_locally_evaluable() {
            return Err(error!(
                "native conditions can't be evaluated in memory",
                native = self.native.join(", ")
            )
            .mark_incorrect_usage());
        }
        Ok(self.conditions.iter().all(|c| c.matches(row, metadata))
            && self
                .search
                .as_deref()
                .is_none_or(|needle| matches_search(needle, row)))
    }

    /// Sort `rows` by this query's order; without one, `rows` keep theirs.
    /// A cell is read by [`cell_get`], as [`matches`](Self::matches) reads
    /// it. An absent or null cell sorts last in both directions, and ties
    /// break on id, so the same rows always come back in the same order.
    pub fn sort_rows(&self, rows: &mut [(String, Rec)], metadata: &VistaMetadata) {
        if let Some((column, dir)) = &self.order {
            rows.sort_by(|(a_id, a), (b_id, b)| {
                compare_rows(column, *dir, metadata, (a_id, a), (b_id, b))
            });
        }
    }

    /// Whether [`matches`](Self::matches) can evaluate this query: it holds
    /// no native conditions.
    pub fn is_locally_evaluable(&self) -> bool {
        self.native.is_empty()
    }
}

/// The order of two `(id, row)` pairs by `column` in direction `dir`, as
/// [`VistaQuery::sort_rows`] sorts them. A null cell counts as absent.
pub(crate) fn compare_rows(
    column: &str,
    dir: SortDirection,
    metadata: &VistaMetadata,
    (a_id, a): (&str, &Rec),
    (b_id, b): (&str, &Rec),
) -> Ordering {
    fn cell<'a>(row: &'a Rec, column: &str, metadata: &VistaMetadata) -> Option<&'a CborValue> {
        cell_get(row, column, metadata).filter(|v| !v.is_null())
    }
    let ordering = match (cell(a, column, metadata), cell(b, column, metadata)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(l), Some(r)) => match dir {
            SortDirection::Ascending => cbor_cmp(l, r),
            SortDirection::Descending => cbor_cmp(r, l),
        },
    };
    ordering.then_with(|| a_id.cmp(b_id))
}

/// Equal values, or an integer and a float of the same number.
fn same(a: &CborValue, b: &CborValue) -> bool {
    let int_and_float = matches!(
        (a, b),
        (CborValue::Integer(_), CborValue::Float(_)) | (CborValue::Float(_), CborValue::Integer(_))
    );
    a == b || (int_and_float && cbor_cmp(a, b) == Ordering::Equal)
}

fn in_set(cell: Option<&CborValue>, operand: &CborValue) -> bool {
    match (cell, operand) {
        (Some(cell), CborValue::Array(items)) => items.iter().any(|item| same(cell, item)),
        _ => false,
    }
}
