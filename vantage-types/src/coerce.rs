//! Lenient write-side conversion behind the generated `coerce_<x>` methods.
//!
//! A value from a form or a script is often loosely typed: `"42"` for an
//! integer, `5` for a float, `"2026-12-10"` for a date. [`LenientValue`] lists
//! the other wire shapes such a value could stand for, and `coerce_<x>` hands
//! each to the type's strict `from_<x>` until one decodes. The parsing that
//! decides validity stays in the type's own decoder; this module only proposes
//! candidates.

use ciborium::Value as CborValue;

/// Alternative encodings of a loosely typed value, most exact first.
///
/// Implemented for the value types of the bundled backends. A
/// `vantage_type_system!` over any other value type gets a strict
/// `coerce_<x>` (the plain `from_<x>`), so external type systems keep
/// compiling unchanged.
pub trait LenientValue: Sized {
    fn lenient_forms(&self) -> Vec<Self>;
}

// Autoref dispatch used by the macro: `(&Probe(&v)).forms()` resolves to
// `ViaLenient` when the value type implements `LenientValue`, and falls back
// to `ViaStrict` (no candidates) when it doesn't. The value type is concrete
// at each macro expansion, so no trait bound leaks into the generated trait.
#[doc(hidden)]
pub struct Probe<'a, V>(pub &'a V);

#[doc(hidden)]
pub trait ViaLenient<V> {
    fn forms(&self) -> Vec<V>;
}

impl<V: LenientValue> ViaLenient<V> for Probe<'_, V> {
    fn forms(&self) -> Vec<V> {
        self.0.lenient_forms()
    }
}

#[doc(hidden)]
pub trait ViaStrict<V> {
    fn forms(&self) -> Vec<V>;
}

impl<V> ViaStrict<V> for &Probe<'_, V> {
    fn forms(&self) -> Vec<V> {
        Vec::new()
    }
}

// CBOR tags the bundled drivers use for typed values.
const TAG_DATETIME_TEXT: u64 = 0;
const TAG_DECIMAL: u64 = 10;
#[cfg(feature = "serde")]
const TAG_SURREAL_DATETIME: u64 = 12;
const TAG_DATE: u64 = 100;
const TAG_TIME: u64 = 101;

impl LenientValue for CborValue {
    fn lenient_forms(&self) -> Vec<Self> {
        match self {
            CborValue::Text(s) => text_forms(s),
            CborValue::Integer(i) => {
                let n = i128::from(*i);
                let mut forms = vec![tag_text(TAG_DECIMAL, n.to_string())];
                let f = n as f64;
                if f as i128 == n && f.abs() < 2f64.powi(53) {
                    forms.push(CborValue::Float(f));
                }
                forms
            }
            CborValue::Float(f) if f.is_finite() => {
                let mut forms = Vec::new();
                if let Some(i) = integral(*f) {
                    forms.push(CborValue::Integer(i.into()));
                }
                forms.push(tag_text(TAG_DECIMAL, f.to_string()));
                forms
            }
            CborValue::Tag(tag, inner) => tagged_forms(*tag, inner),
            _ => Vec::new(),
        }
    }
}

fn text_forms(s: &str) -> Vec<CborValue> {
    let mut forms = Vec::new();
    match s {
        "true" => forms.push(CborValue::Bool(true)),
        "false" => forms.push(CborValue::Bool(false)),
        _ => {}
    }
    if let Ok(i) = s.parse::<i128>()
        && let Ok(i) = ciborium::value::Integer::try_from(i)
    {
        forms.push(CborValue::Integer(i));
    }
    if let Ok(f) = s.parse::<f64>()
        && f.is_finite()
    {
        forms.push(tag_text(TAG_DECIMAL, s.to_string()));
        forms.push(CborValue::Float(f));
    }
    if is_date(s) {
        forms.push(tag_text(TAG_DATE, s.to_string()));
        forms.extend(midnight_instant_forms(s));
    } else if is_date(s.get(..10).unwrap_or("")) {
        if let Some(date) = midnight_date(s) {
            forms.push(tag_text(TAG_DATE, date.to_string()));
        }
        forms.extend(instant_forms(s));
    }
    if is_time(s) {
        forms.push(tag_text(TAG_TIME, s.to_string()));
    }
    forms
}

fn tagged_forms(tag: u64, inner: &CborValue) -> Vec<CborValue> {
    #[cfg(feature = "serde")]
    if tag == TAG_SURREAL_DATETIME {
        let Some(rfc) = crate::cbor_json::tag12_to_rfc3339(inner) else {
            return Vec::new();
        };
        let mut forms = vec![tag_text(TAG_DATETIME_TEXT, rfc.clone())];
        if let Some(date) = midnight_date(&rfc) {
            forms.push(tag_text(TAG_DATE, date.to_string()));
        }
        forms.push(CborValue::Text(rfc));
        return forms;
    }
    let CborValue::Text(s) = inner else {
        return Vec::new();
    };
    let mut forms = Vec::new();
    match tag {
        TAG_DATETIME_TEXT => {
            if let Some(date) = midnight_date(s) {
                forms.push(tag_text(TAG_DATE, date.to_string()));
            }
            forms.push(CborValue::Text(s.clone()));
            #[cfg(feature = "serde")]
            forms.extend(crate::cbor_json::rfc3339_to_tag12(s));
        }
        TAG_DATE => {
            forms.push(CborValue::Text(s.clone()));
            forms.extend(midnight_instant_forms(s));
        }
        TAG_TIME => forms.push(CborValue::Text(s.clone())),
        TAG_DECIMAL => {
            if let Ok(i) = s.parse::<i128>()
                && let Ok(i) = ciborium::value::Integer::try_from(i)
            {
                forms.push(CborValue::Integer(i));
            }
            if let Ok(f) = s.parse::<f64>()
                && f.is_finite()
            {
                forms.push(CborValue::Float(f));
            }
            forms.push(CborValue::Text(s.clone()));
        }
        _ => {}
    }
    forms
}

// Date-only text as an instant: midnight UTC.
fn midnight_instant_forms(date: &str) -> Vec<CborValue> {
    instant_forms(&format!("{date}T00:00:00Z"))
}

fn instant_forms(s: &str) -> Vec<CborValue> {
    #[allow(unused_mut)]
    let mut forms = vec![tag_text(TAG_DATETIME_TEXT, s.to_string())];
    #[cfg(feature = "serde")]
    forms.extend(crate::cbor_json::rfc3339_to_tag12(s));
    forms
}

fn tag_text(tag: u64, s: String) -> CborValue {
    CborValue::Tag(tag, Box::new(CborValue::Text(s)))
}

fn integral(f: f64) -> Option<i64> {
    // `i64::MAX as f64` rounds up to 2^63, so the upper bound is exclusive.
    (f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64).then_some(f as i64)
}

/// `YYYY-MM-DD` with a plausible month and day; the type's decoder does the
/// calendar check.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
        && (1..=12).contains(&two_digits(&b[5..7]))
        && (1..=31).contains(&two_digits(&b[8..10]))
}

/// `HH:MM` or `HH:MM:SS`, optionally with a fraction.
fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    let hm = b.len() >= 5
        && b[2] == b':'
        && [0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit())
        && two_digits(&b[0..2]) < 24
        && two_digits(&b[3..5]) < 60;
    if !hm {
        return false;
    }
    match &b[5..] {
        [] => true,
        [b':', s1, s2, rest @ ..] => {
            s1.is_ascii_digit()
                && s2.is_ascii_digit()
                && two_digits(&[*s1, *s2]) < 60
                && match rest {
                    [] => true,
                    [b'.', frac @ ..] => !frac.is_empty() && frac.iter().all(u8::is_ascii_digit),
                    _ => false,
                }
        }
        _ => false,
    }
}

fn two_digits(b: &[u8]) -> u8 {
    (b[0] - b'0') * 10 + (b[1] - b'0')
}

/// The date part of a datetime at exactly midnight UTC (or with no offset).
/// A later time, or another offset, is not a date: truncating it would depend
/// on the timezone.
fn midnight_date(s: &str) -> Option<&str> {
    let date = s.get(..10).filter(|d| is_date(d))?;
    let rest = s[10..].strip_prefix(['T', ' '])?.strip_prefix("00:00:00")?;
    let rest = match rest.strip_prefix('.') {
        Some(frac) => frac.trim_start_matches('0'),
        None => rest,
    };
    matches!(rest, "" | "Z" | "+00:00" | "+00" | "+0000").then_some(date)
}

#[cfg(feature = "serde")]
impl LenientValue for serde_json::Value {
    fn lenient_forms(&self) -> Vec<Self> {
        use serde_json::{Number, Value as JsonValue};
        match self {
            JsonValue::String(s) => {
                let mut forms = Vec::new();
                match s.as_str() {
                    "true" => forms.push(JsonValue::Bool(true)),
                    "false" => forms.push(JsonValue::Bool(false)),
                    _ => {}
                }
                if let Ok(i) = s.parse::<i64>() {
                    forms.push(JsonValue::Number(i.into()));
                } else if let Ok(u) = s.parse::<u64>() {
                    forms.push(JsonValue::Number(u.into()));
                }
                if let Some(n) = s.parse::<f64>().ok().and_then(Number::from_f64) {
                    forms.push(JsonValue::Number(n));
                }
                forms
            }
            JsonValue::Number(n) => {
                if let Some(f) = n.as_f64().filter(|_| n.is_f64()) {
                    integral(f)
                        .map(|i| vec![JsonValue::Number(i.into())])
                        .unwrap_or_default()
                } else {
                    let i = n.as_i64().map(|i| i as i128).or(n.as_u64().map(i128::from));
                    i.filter(|i| i.unsigned_abs() < 1 << 53)
                        .and_then(|i| Number::from_f64(i as f64))
                        .map(|n| vec![JsonValue::Number(n)])
                        .unwrap_or_default()
                }
            }
            _ => Vec::new(),
        }
    }
}
