//! One total order over CBOR cells, shared by every in-memory evaluation.

use std::cmp::Ordering;

use ciborium::Value as CborValue;

/// A total order over the CBOR values rows carry.
///
/// - Integers compare exactly. An integer and a float, or two floats,
///   compare numerically through `f64::total_cmp`, so `3` sorts below
///   `4.95`.
///   - `-0.0` is taken as `0.0`, so it equals `0` and `0.0`.
///   - NaN is ordered rather than refused: a positive NaN above every
///     number, a negative one below, and a NaN equal to itself.
///   - An integer beyond 2^53 is rounded to the nearest float to meet a
///     float, so it can equal a float a few units away.
/// - Text compares by code point, bool `false` before `true`.
/// - Any other pair, of mixed or compound types, compares by its debug
///   text. That claims no meaning, but keeps sorting deterministic.
///
/// An absent cell is the caller's to place; [`VistaQuery::sort_rows`]
/// sorts it, and a null, last in both directions.
///
/// [`VistaQuery::sort_rows`]: crate::VistaQuery::sort_rows
pub fn cbor_cmp(a: &CborValue, b: &CborValue) -> Ordering {
    match (a, b) {
        (CborValue::Integer(l), CborValue::Integer(r)) => i128::from(*l).cmp(&i128::from(*r)),
        (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
        (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
        _ => match (number(a), number(b)) {
            (Some(l), Some(r)) => l.total_cmp(&r),
            _ => format!("{a:?}").cmp(&format!("{b:?}")),
        },
    }
}

/// `value` as a float, `-0.0` folded into `0.0`.
fn number(value: &CborValue) -> Option<f64> {
    let n = match value {
        CborValue::Integer(i) => i128::from(*i) as f64,
        CborValue::Float(f) => *f,
        _ => return None,
    };
    Some(if n == 0.0 { 0.0 } else { n })
}
