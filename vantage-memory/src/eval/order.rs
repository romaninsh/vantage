//! Multi-key stable ordering. Nulls sort first ascending; descending
//! reverses the whole comparison.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_vista::SortDirection;

use super::compare::{cmp_values, lookup};
use crate::Row;

fn rank(v: Option<&CborValue>) -> u8 {
    match v {
        None | Some(CborValue::Null) => 0,
        Some(CborValue::Bool(_)) => 1,
        Some(CborValue::Integer(_) | CborValue::Float(_)) => 2,
        Some(CborValue::Text(_)) => 3,
        Some(_) => 4,
    }
}

fn cmp_cells(a: Option<&CborValue>, b: Option<&CborValue>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => cmp_values(x, y).unwrap_or_else(|| rank(a).cmp(&rank(b))),
        _ => rank(a).cmp(&rank(b)),
    }
}

pub fn sort_rows(rows: &mut [(String, Row)], order: &[(String, SortDirection)]) {
    if order.is_empty() {
        return;
    }
    rows.sort_by(|(_, a), (_, b)| {
        for (path, dir) in order {
            let o = cmp_cells(lookup(a, path), lookup(b, path));
            let o = match dir {
                SortDirection::Ascending => o,
                SortDirection::Descending => o.reverse(),
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
}
