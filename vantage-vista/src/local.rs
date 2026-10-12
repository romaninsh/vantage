//! Evaluating conditions over rows held in memory, for the in-crate shells
//! that keep their rows themselves (the contained shell and `MockShell`).

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::{filter::FilterOp, filter::operand_text, query::Condition};

/// Whether `record` satisfies `condition`. A missing cell matches only `Ne`
/// and `NotInSet`.
pub(crate) fn matches(condition: &Condition, record: &Record<CborValue>) -> bool {
    let cell = record.get(&condition.column);
    let operand = &condition.value;
    match condition.op {
        FilterOp::Eq => cell == Some(operand),
        FilterOp::Ne => cell != Some(operand),
        FilterOp::InSet => in_set(cell, operand),
        FilterOp::NotInSet => !in_set(cell, operand),
        FilterOp::Like => cell.is_some_and(|c| {
            operand_text(c)
                .to_lowercase()
                .contains(&operand_text(operand).to_lowercase())
        }),
        op => cell.is_some_and(|c| op.matches_ordering(cbor_cmp(Some(c), Some(operand)))),
    }
}

fn in_set(cell: Option<&CborValue>, operand: &CborValue) -> bool {
    match (cell, operand) {
        (Some(cell), CborValue::Array(items)) => items.contains(cell),
        _ => false,
    }
}

/// A total order over the CBOR scalars in-memory rows carry. Integers and
/// floats compare numerically with each other. Other mixed or unknown types
/// fall back to comparing their debug text, which keeps sorting deterministic
/// without claiming they are comparable.
pub(crate) fn cbor_cmp(a: Option<&CborValue>, b: Option<&CborValue>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(lhs), Some(rhs)) => match (lhs, rhs) {
            (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
            (CborValue::Integer(l), CborValue::Integer(r)) => i128::from(*l).cmp(&i128::from(*r)),
            (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
            _ => match (number(lhs), number(rhs)) {
                (Some(l), Some(r)) => l.total_cmp(&r),
                _ => format!("{lhs:?}").cmp(&format!("{rhs:?}")),
            },
        },
    }
}

fn number(value: &CborValue) -> Option<f64> {
    match value {
        CborValue::Integer(i) => Some(i128::from(*i) as f64),
        CborValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Whether any text cell of `record` contains `needle`, case-insensitively.
#[cfg(any(test, feature = "mocks"))]
pub(crate) fn matches_search(needle: &str, record: &Record<CborValue>) -> bool {
    let needle = needle.to_lowercase();
    record.values().any(|v| match v {
        CborValue::Text(s) => s.to_lowercase().contains(&needle),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Record<CborValue> {
        [
            ("n".to_string(), CborValue::Integer(5.into())),
            ("s".to_string(), CborValue::Text("Straße".into())),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn every_operator_evaluates() {
        let r = row();
        let n = |op, v: i64| matches(&Condition::new("n", op, v), &r);
        assert!(n(FilterOp::Eq, 5) && !n(FilterOp::Eq, 4));
        assert!(n(FilterOp::Ne, 4) && n(FilterOp::Gt, 4) && n(FilterOp::Gte, 5));
        assert!(n(FilterOp::Lt, 6) && n(FilterOp::Lte, 5) && !n(FilterOp::Lt, 5));
        let set = CborValue::Array(vec![1.into(), 5.into()]);
        assert!(matches(
            &Condition::new("n", FilterOp::InSet, set.clone()),
            &r
        ));
        assert!(!matches(&Condition::new("n", FilterOp::NotInSet, set), &r));
        assert!(matches(&Condition::new("s", FilterOp::Like, "STRA"), &r));
        assert!(matches(&Condition::new("missing", FilterOp::Ne, 1), &r));
    }

    #[test]
    fn integers_and_floats_compare_numerically() {
        let int_row: Record<CborValue> = [("price".to_string(), CborValue::Integer(3.into()))]
            .into_iter()
            .collect();
        let float_row: Record<CborValue> = [("price".to_string(), CborValue::Float(4.95))]
            .into_iter()
            .collect();
        let p =
            |op, v: CborValue, r: &Record<CborValue>| matches(&Condition::new("price", op, v), r);
        assert!(p(FilterOp::Lt, CborValue::Float(4.9), &int_row));
        assert!(!p(FilterOp::Lt, CborValue::Float(4.9), &float_row));
        assert!(p(FilterOp::Gt, CborValue::Integer(4.into()), &float_row));
        assert!(!p(FilterOp::Gt, CborValue::Float(3.5), &int_row));
        assert!(p(FilterOp::Gte, CborValue::Float(3.0), &int_row));
        assert!(!p(FilterOp::Gte, CborValue::Integer(5.into()), &float_row));
    }
}
