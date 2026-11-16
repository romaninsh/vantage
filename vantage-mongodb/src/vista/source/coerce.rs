//! Value-level typing for writes arriving as CBOR.
//!
//! The CBOR surface reads `ObjectId`, `DateTime` and `Decimal128` as text, so
//! a row read and written back would otherwise store those fields as strings.
//! The rules, applied per column by the `write` module:
//!
//! - on replace and patch, a value equal to the stored value's CBOR rendering
//!   is written back as stored, so data that does not fit the column's type
//!   (a legacy string in an `object_id` column, a DateTime outside the RFC
//!   3339 range, which renders as `""`) survives a round trip unchanged;
//! - a column declared with one of those types parses each outgoing value as
//!   that type ([`declared`]), element-wise through arrays; `null` and `""`
//!   store null (unless the stored value already renders as `""`), anything
//!   else that does not parse is refused. Over a stored array of the same
//!   length, unchanged elements keep their stored value ([`declared_over`]);
//! - on replace and patch, an undeclared column takes the type of the value
//!   stored at its path when the outgoing value parses as that type
//!   ([`preserve`]); otherwise the value is written as its plain CBOR → BSON
//!   conversion, so text that does not parse over a stored `ObjectId` becomes
//!   a String;
//! - inserts and imports have no stored row, so only declared columns are
//!   typed.
//!
//! DateTime has millisecond precision: finer fractions in RFC 3339 text or
//! epoch seconds are dropped.
//!
//! `date` and `timestamp` columns are stored as a DateTime as well, but they
//! take more text than RFC 3339 and read back typed, not as text:
//!
//! - a write takes a date (`2026-12-10`, midnight UTC), a naive datetime
//!   with a `T` or a space, seconds optional, a fraction allowed (UTC is
//!   assumed), RFC 3339 text with `Z` or an offset, CBOR tag 0 or 100 around
//!   any of those, and tag 1 epoch seconds. A `date` column stores midnight
//!   UTC and refuses an instant with a time of day;
//! - a read returns `Tag(100, "YYYY-MM-DD")` for `date` (the UTC date of the
//!   stored DateTime) and `Tag(0, RFC 3339 in UTC with Z)` for `timestamp`
//!   ([`read`]). Text stored in such a column reads through the write's
//!   parser and is not rewritten; anything that is no date fails the read.
//!   The round-trip rule above compares against this typed rendering;
//! - a condition value is typed as a write is ([`declared`]).

use std::any::type_name;
use std::str::FromStr;

use bson::oid::ObjectId;
use bson::{Bson, DateTime, Decimal128, Document};
use chrono::{NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use ciborium::Value as CborValue;
use vantage_core::{Result, error};

use crate::vista::cbor::{bson_to_cbor, cbor_to_bson};

const DAY_MILLIS: i64 = 86_400_000;

/// Naive datetime text; SQLite's `timestamp` column takes the same forms.
const NAIVE_FORMATS: [&str; 6] = [
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M",
];

/// The column types a write parses values into: the BSON types the CBOR
/// surface reads as text, and the two that read back typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BsonKind {
    ObjectId,
    DateTime,
    Decimal,
    Date,
    Timestamp,
}

impl BsonKind {
    /// The kind a table column's Rust type stands for, if any.
    pub(crate) fn of_column_type(column_type: &str) -> Option<Self> {
        if column_type == type_name::<ObjectId>() {
            Some(Self::ObjectId)
        } else if column_type == type_name::<DateTime>() {
            Some(Self::DateTime)
        } else if column_type == type_name::<Decimal128>() {
            Some(Self::Decimal)
        } else if column_type == type_name::<NaiveDate>() {
            Some(Self::Date)
        } else if column_type == type_name::<chrono::DateTime<Utc>>() {
            Some(Self::Timestamp)
        } else {
            None
        }
    }

    /// Whether a read returns this kind typed (see [`read`]).
    pub(crate) fn reads_typed(self) -> bool {
        matches!(self, Self::Date | Self::Timestamp)
    }

    fn of_stored(value: &Bson) -> Option<Self> {
        match value {
            Bson::ObjectId(_) => Some(Self::ObjectId),
            Bson::DateTime(_) => Some(Self::DateTime),
            Bson::Decimal128(_) => Some(Self::Decimal),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ObjectId => "object_id",
            Self::DateTime => "datetime",
            Self::Decimal => "decimal",
            Self::Date => "date",
            Self::Timestamp => "timestamp",
        }
    }

    /// `value` as this kind. ObjectId: hex text. DateTime: RFC 3339 text,
    /// CBOR tag 0 (date text) or tag 1 (epoch seconds). Decimal128: numeric
    /// text, a number, CBOR tag 2/3 (bignum) or tag 4 (decimal fraction).
    /// Date and Timestamp: see [`instant_millis`].
    fn parse(self, value: &CborValue) -> Option<Bson> {
        match (self, value) {
            (Self::ObjectId, CborValue::Text(s)) => s.parse().ok().map(Bson::ObjectId),
            (Self::DateTime, CborValue::Text(s)) => rfc3339(s),
            (Self::DateTime, CborValue::Tag(0, inner)) => match inner.as_ref() {
                CborValue::Text(s) => rfc3339(s),
                _ => None,
            },
            (Self::DateTime, CborValue::Tag(1, inner)) => epoch_millis(inner).map(datetime),
            (Self::Date, value) => instant_millis(value)
                .filter(|millis| millis.rem_euclid(DAY_MILLIS) == 0)
                .map(datetime),
            (Self::Timestamp, value) => instant_millis(value).map(datetime),
            (Self::Decimal, CborValue::Text(s)) => decimal(s.trim()),
            (Self::Decimal, CborValue::Float(f)) if f.is_finite() => decimal(&f.to_string()),
            (Self::Decimal, CborValue::Tag(4, fraction)) => decimal_fraction(fraction),
            (Self::Decimal, integer) => decimal(&integer_text(integer)?),
            _ => None,
        }
    }

    /// `value` as a read returns it for a `date` or `timestamp` column;
    /// `None` when it is neither a DateTime, text a write would take, nor
    /// no value (null, `""`).
    fn typed(self, value: &Bson) -> Option<CborValue> {
        let millis = match value {
            Bson::Null | Bson::Undefined => return Some(CborValue::Null),
            Bson::String(s) if s.is_empty() => return Some(CborValue::Null),
            Bson::Array(items) => {
                return items
                    .iter()
                    .map(|item| self.typed(item))
                    .collect::<Option<Vec<_>>>()
                    .map(CborValue::Array);
            }
            Bson::DateTime(stored) => stored.timestamp_millis(),
            Bson::String(s) => text_millis(s)
                .filter(|millis| self != Self::Date || millis.rem_euclid(DAY_MILLIS) == 0)?,
            _ => return None,
        };
        let instant = chrono::DateTime::<Utc>::from_timestamp_millis(millis)?;
        let (tag, text) = match self {
            Self::Date => (100, instant.format("%Y-%m-%d").to_string()),
            _ => (0, instant.to_rfc3339_opts(SecondsFormat::AutoSi, true)),
        };
        Some(CborValue::Tag(tag, Box::new(CborValue::Text(text))))
    }
}

/// `value` as a read returns it for a column of `kind`: typed for `date` and
/// `timestamp`, the plain CBOR rendering otherwise. `None` when a `date` or
/// `timestamp` column holds something that is no date.
pub(crate) fn rendered(kind: Option<BsonKind>, value: &Bson) -> Option<CborValue> {
    match kind {
        Some(kind) if kind.reads_typed() => kind.typed(value),
        _ => Some(bson_to_cbor(value)),
    }
}

/// [`rendered`], with a value that is no date as an error naming the column
/// and the value.
pub(crate) fn read(
    collection: &str,
    column: &str,
    kind: Option<BsonKind>,
    value: &Bson,
) -> Result<CborValue> {
    rendered(kind, value).ok_or_else(|| {
        let expected = kind.map_or("", BsonKind::name);
        let held = match value {
            Bson::String(s) => format!("{s:?}"),
            other => other.to_string(),
        };
        error!(
            format!("`{column}` of `{collection}` holds {held}, which is not a `{expected}`"),
            collection = collection,
            column = column,
            expected = expected,
            value = held
        )
    })
}

fn rfc3339(s: &str) -> Option<Bson> {
    DateTime::parse_rfc3339_str(s).ok().map(Bson::DateTime)
}

fn datetime(millis: i64) -> Bson {
    Bson::DateTime(DateTime::from_millis(millis))
}

/// The instant `value` names, in epoch milliseconds: text, CBOR tag 0 or 100
/// around text, or tag 1 epoch seconds.
fn instant_millis(value: &CborValue) -> Option<i64> {
    match value {
        CborValue::Text(s) => text_millis(s),
        CborValue::Tag(0 | 100, inner) => match inner.as_ref() {
            CborValue::Text(s) => text_millis(s),
            _ => None,
        },
        CborValue::Tag(1, inner) => epoch_millis(inner),
        _ => None,
    }
}

/// Text with `Z` or an offset, a naive datetime, or a date: the last two are
/// UTC, a date its midnight.
fn text_millis(s: &str) -> Option<i64> {
    if let Ok(instant) = s.parse::<chrono::DateTime<Utc>>() {
        return Some(instant.timestamp_millis());
    }
    let naive = NAIVE_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(s, format).ok())
        .or_else(|| {
            let date = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
            (s.len() == 10).then_some(date)?.and_hms_opt(0, 0, 0)
        })?;
    Some(naive.and_utc().timestamp_millis())
}

fn decimal(s: &str) -> Option<Bson> {
    Decimal128::from_str(s).ok().map(Bson::Decimal128)
}

/// The decimal digits of a CBOR integer or tag 2/3 bignum (up to 128 bits).
fn integer_text(value: &CborValue) -> Option<String> {
    match value {
        CborValue::Integer(i) => Some(i128::from(*i).to_string()),
        CborValue::Tag(tag @ (2 | 3), inner) => {
            let CborValue::Bytes(bytes) = inner.as_ref() else {
                return None;
            };
            let digits = bytes.iter().skip_while(|b| **b == 0).collect::<Vec<_>>();
            if digits.len() > 16 {
                return None;
            }
            let n = digits.iter().fold(0u128, |n, b| (n << 8) | u128::from(**b));
            Some(if *tag == 2 {
                n.to_string()
            } else {
                // Tag 3 encodes -1 - n.
                format!("-{}", n.checked_add(1)?)
            })
        }
        _ => None,
    }
}

/// Tag 4: `[exponent, mantissa]`, the value `mantissa × 10^exponent`.
fn decimal_fraction(value: &CborValue) -> Option<Bson> {
    let CborValue::Array(parts) = value else {
        return None;
    };
    let [CborValue::Integer(exponent), mantissa] = parts.as_slice() else {
        return None;
    };
    decimal(&format!(
        "{}e{}",
        integer_text(mantissa)?,
        i128::from(*exponent)
    ))
}

/// Epoch seconds as milliseconds.
fn epoch_millis(value: &CborValue) -> Option<i64> {
    match value {
        CborValue::Integer(i) => i64::try_from(*i).ok()?.checked_mul(1000),
        CborValue::Float(f) => {
            let millis = (f * 1000.0).round();
            if !millis.is_finite() || millis < i64::MIN as f64 || millis >= i64::MAX as f64 {
                return None;
            }
            Some(millis as i64)
        }
        _ => None,
    }
}

/// `value` for a column declared as `kind`.
pub(crate) fn declared(column: &str, kind: BsonKind, value: &CborValue) -> Result<Bson> {
    match value {
        CborValue::Null => Ok(Bson::Null),
        CborValue::Text(s) if s.is_empty() => Ok(Bson::Null),
        CborValue::Array(items) => items
            .iter()
            .map(|item| declared(column, kind, item))
            .collect::<Result<Vec<_>>>()
            .map(Bson::Array),
        other => kind.parse(other).ok_or_else(|| {
            error!(
                "value does not match the column type",
                column = column,
                expected = kind.name()
            )
            .mark_conflict()
        }),
    }
}

/// `value` for a column declared as `kind`, written over `stored`. Over a
/// stored array of the same length, an element equal to its stored
/// counterpart's CBOR rendering keeps the stored element; the others go
/// through [`declared`]. Any other shape is [`declared`] as a whole.
pub(crate) fn declared_over(
    column: &str,
    kind: BsonKind,
    stored: Option<&Bson>,
    value: &CborValue,
) -> Result<Bson> {
    match (stored, value) {
        (Some(Bson::Array(old)), CborValue::Array(new)) if old.len() == new.len() => old
            .iter()
            .zip(new)
            .map(|(o, n)| {
                if rendered(Some(kind), o).as_ref() == Some(n) {
                    Ok(o.clone())
                } else {
                    declared(column, kind, n)
                }
            })
            .collect::<Result<Vec<_>>>()
            .map(Bson::Array),
        _ => declared(column, kind, value),
    }
}

/// `value` written over `stored`, keeping the stored type where it parses.
/// Recurses through sub-documents by key and through arrays by position
/// (only when both arrays have the same length); a stored `Int32` stays one
/// while the outgoing integer fits.
pub(crate) fn preserve(stored: &Bson, value: &CborValue) -> Bson {
    match (stored, value) {
        (Bson::Array(old), CborValue::Array(new)) if old.len() == new.len() => {
            Bson::Array(old.iter().zip(new).map(|(o, n)| preserve(o, n)).collect())
        }
        (Bson::Document(old), CborValue::Map(entries)) => {
            let mut doc = Document::new();
            for (key, item) in entries {
                let CborValue::Text(key) = key else {
                    // Non-text keys take the plain conversion's spelling.
                    return cbor_to_bson(value);
                };
                let item = match old.get(key) {
                    Some(o) => preserve(o, item),
                    None => cbor_to_bson(item),
                };
                doc.insert(key.clone(), item);
            }
            Bson::Document(doc)
        }
        (Bson::Int32(_), CborValue::Integer(i)) => match i32::try_from(*i) {
            Ok(n) => Bson::Int32(n),
            Err(_) => cbor_to_bson(value),
        },
        _ => BsonKind::of_stored(stored)
            .and_then(|kind| kind.parse(value))
            .unwrap_or_else(|| cbor_to_bson(value)),
    }
}
