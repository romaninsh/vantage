use ciborium::Value as CborValue;
use vantage_types::Record;

pub(crate) fn matches_conditions(rec: &Record<CborValue>, conds: &[(String, CborValue)]) -> bool {
    conds.iter().all(|(col, expected)| match rec.get(col) {
        Some(v) => cbor_eq(v, expected),
        None => false,
    })
}

pub(crate) fn matches_search(rec: &Record<CborValue>, needle: Option<&str>) -> bool {
    let Some(needle) = needle else {
        return true;
    };
    let needle_lc = needle.to_lowercase();
    rec.values().any(|v| match v {
        CborValue::Text(s) => s.to_lowercase().contains(&needle_lc),
        _ => false,
    })
}

pub(crate) fn cbor_eq(a: &CborValue, b: &CborValue) -> bool {
    match (a, b) {
        (CborValue::Text(x), CborValue::Text(y)) => x == y,
        (CborValue::Integer(x), CborValue::Integer(y)) => x == y,
        (CborValue::Bool(x), CborValue::Bool(y)) => x == y,
        // Float and the rest fall back to format-string compare. Good
        // enough for v1's hand-rolled filter.
        _ => format!("{a:?}") == format!("{b:?}"),
    }
}

pub(crate) fn cbor_cmp(a: Option<&CborValue>, b: Option<&CborValue>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(lhs), Some(rhs)) => match (lhs, rhs) {
            (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
            (CborValue::Integer(l), CborValue::Integer(r)) => i128::from(*l).cmp(&i128::from(*r)),
            (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
            _ => format!("{lhs:?}").cmp(&format!("{rhs:?}")),
        },
    }
}
