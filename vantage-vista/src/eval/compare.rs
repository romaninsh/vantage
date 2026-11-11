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
/// - Two datetimes compare by instant: a SurrealDB epoch pair (`Tag(12)`)
///   or a CBOR standard datetime (`Tag(0, Text)`, RFC 3339), in any mix and
///   any offset.
/// - A CBOR standard datetime against anything else is taken as its text,
///   so it orders against, and equals, dates a source stores as text.
///   RFC 3339 strings with the same offset sort lexically.
/// - Any other pair, of mixed or compound types, compares by its debug
///   text. That claims no meaning, but keeps sorting deterministic.
///
/// An absent cell is the caller's to place; [`VistaQuery::sort_rows`]
/// sorts it, and a null, last in both directions.
///
/// [`VistaQuery::sort_rows`]: crate::VistaQuery::sort_rows
pub fn cbor_cmp(a: &CborValue, b: &CborValue) -> Ordering {
    if let (Some(l), Some(r)) = (instant(a), instant(b)) {
        return l.cmp(&r);
    }
    let (a, b) = (datetime_as_text(a), datetime_as_text(b));
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

/// The text inside a CBOR standard datetime (`Tag(0, Text)`), or `value`
/// itself. Twin: `vantage-memory/src/eval/compare.rs`.
pub(super) fn datetime_as_text(value: &CborValue) -> &CborValue {
    match value {
        CborValue::Tag(0, inner) if matches!(inner.as_ref(), CborValue::Text(_)) => inner,
        _ => value,
    }
}

/// The moment a datetime value stands for, as `(seconds, nanoseconds)` since
/// the Unix epoch. `None` for anything that is not a datetime, or whose text
/// is not RFC 3339.
fn instant(value: &CborValue) -> Option<(i64, u32)> {
    match value {
        CborValue::Tag(12, inner) => {
            let CborValue::Array(parts) = inner.as_ref() else {
                return None;
            };
            let part = |i: usize| match parts.get(i) {
                Some(CborValue::Integer(n)) => i64::try_from(i128::from(*n)).ok(),
                _ => None,
            };
            let nanos = if parts.len() > 1 { part(1)? } else { 0 };
            Some((part(0)?, u32::try_from(nanos).ok()?))
        }
        CborValue::Tag(0, inner) => match inner.as_ref() {
            CborValue::Text(text) => rfc3339(text),
            _ => None,
        },
        _ => None,
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)` as an instant.
fn rfc3339(text: &str) -> Option<(i64, u32)> {
    let bytes = text.as_bytes();
    let digits = |from: usize, len: usize| -> Option<i64> {
        let part = bytes.get(from..from + len)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let at = |i: usize, allowed: &[u8]| bytes.get(i).is_some_and(|c| allowed.contains(c));
    if !(at(4, b"-") && at(7, b"-") && at(10, b"Tt ") && at(13, b":") && at(16, b":")) {
        return None;
    }
    let (year, month, day) = (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?);
    let (hour, minute, second) = (digits(11, 2)?, digits(14, 2)?, digits(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let mut end = 19;
    let mut nanos = 0;
    if at(end, b".") {
        let start = end + 1;
        end = start;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start {
            return None;
        }
        let kept = &bytes[start..end.min(start + 9)];
        nanos = kept.iter().fold(0u32, |n, d| n * 10 + u32::from(d - b'0'));
        nanos *= 10u32.pow(9 - kept.len() as u32);
    }
    let offset = match bytes.get(end..)? {
        b"Z" | b"z" => 0,
        [sign @ (b'+' | b'-'), ..] if bytes.len() == end + 6 && at(end + 3, b":") => {
            let secs = digits(end + 1, 2)? * 3600 + digits(end + 4, 2)? * 60;
            if *sign == b'+' { secs } else { -secs }
        }
        _ => return None,
    };
    // Days since 1970-01-01 for a proleptic Gregorian date.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some((
        days * 86_400 + hour * 3600 + minute * 60 + second - offset,
        nanos,
    ))
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
