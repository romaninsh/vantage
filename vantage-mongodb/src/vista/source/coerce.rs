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
//! `date`, `timestamp` and `time` columns take every input form of
//! [`vantage_types::temporal`] and read back in its carrier forms:
//!
//! - `timestamp` stores a DateTime. A write takes any instant the `as_instant`
//!   reader takes, tag 1 epoch seconds, or a date (its midnight UTC);
//! - `date` stores a DateTime at midnight UTC. A write takes any date the
//!   `as_date` reader takes, an instant at exactly midnight UTC, or tag 1
//!   epoch seconds at midnight; an instant with a time of day is refused;
//! - `time` stores the carrier text `HH:MM:SS.ffffff`, cut to milliseconds
//!   like a DateTime. BSON has no time of day;
//! - a read returns `Tag(0)` instants, `Tag(1004)` dates (only from a DateTime
//!   at exactly midnight UTC; a time of day fails the read) and `Tag(101)` times ([`read`]). Text another program
//!   stored in such a column reads through the write's parser and is not
//!   rewritten; anything that does not parse fails the read. The round-trip
//!   rule above compares against this typed rendering;
//! - a condition value is typed as a write is ([`declared`]), so it compares
//!   by value: a DateTime, or the fixed-width time text.

use std::any::type_name;
use std::str::FromStr;

use bson::oid::ObjectId;
use bson::{Bson, DateTime, Decimal128, Document};
use chrono::{NaiveDate, NaiveTime, Timelike, Utc};
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::temporal;

use crate::vista::cbor::{bson_to_cbor, cbor_to_bson};

/// The column types a write parses values into: the BSON types the CBOR
/// surface reads as text, and the three that read back typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BsonKind {
    ObjectId,
    DateTime,
    Decimal,
    Date,
    Timestamp,
    Time,
    /// YAML `json`: a map or an array, stored as one sub-document or array.
    Json,
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
        } else if column_type == type_name::<NaiveTime>() {
            Some(Self::Time)
        } else if column_type == type_name::<serde_json::Value>() {
            Some(Self::Json)
        } else {
            None
        }
    }

    /// Whether a read returns this kind typed (see [`read`]).
    pub(crate) fn reads_typed(self) -> bool {
        matches!(self, Self::Date | Self::Timestamp | Self::Time)
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
            Self::Time => "time",
            Self::Json => "json",
        }
    }

    /// `value` as this kind. ObjectId: hex text. DateTime: RFC 3339 text,
    /// CBOR tag 0 (date text) or tag 1 (epoch seconds). Decimal128: numeric
    /// text, a number, CBOR tag 2/3 (bignum) or tag 4 (decimal fraction).
    /// Date, Timestamp and Time: see [`date_of`], [`instant_of`] and the
    /// `as_time` reader.
    fn parse(self, value: &CborValue) -> Option<Bson> {
        match (self, value) {
            (Self::ObjectId, CborValue::Text(s)) => s.parse().ok().map(Bson::ObjectId),
            (Self::DateTime, CborValue::Text(s)) => rfc3339(s),
            (Self::DateTime, CborValue::Tag(0, inner)) => match inner.as_ref() {
                CborValue::Text(s) => rfc3339(s),
                _ => None,
            },
            (Self::DateTime, CborValue::Tag(1, inner)) => epoch_millis(inner).map(datetime),
            (Self::Date, value) => date_of(value).map(|day| datetime(midnight_millis(day))),
            (Self::Timestamp, value) => instant_of(value).map(|at| datetime(at.timestamp_millis())),
            (Self::Time, value) => temporal::as_time(value)
                .map(|time| Bson::String(temporal::format_time(&to_millis(time)))),
            (Self::Decimal, CborValue::Text(s)) => decimal(s.trim()),
            (Self::Decimal, CborValue::Float(f)) if f.is_finite() => decimal(&f.to_string()),
            (Self::Decimal, CborValue::Tag(4, fraction)) => decimal_fraction(fraction),
            (Self::Decimal, integer) => decimal(&integer_text(integer)?),
            (Self::Json, CborValue::Map(_) | CborValue::Array(_)) => Some(cbor_to_bson(value)),
            (Self::Json, CborValue::Text(s)) => {
                match serde_json::from_str::<serde_json::Value>(s).ok()? {
                    json @ (serde_json::Value::Object(_) | serde_json::Value::Array(_)) => {
                        Some(cbor_to_bson(&vantage_types::cbor_json::json_to_cbor(json)))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// `value` as a read returns it for a `date`, `timestamp` or `time`
    /// column; `None` when it is neither a DateTime (`date`, `timestamp`),
    /// text a write would take, nor no value (null, `""`).
    fn typed(self, value: &Bson) -> Option<CborValue> {
        match (self, value) {
            (_, Bson::Null | Bson::Undefined) => Some(CborValue::Null),
            (_, Bson::String(s)) if s.is_empty() => Some(CborValue::Null),
            (_, Bson::Array(items)) => items
                .iter()
                .map(|item| self.typed(item))
                .collect::<Option<Vec<_>>>()
                .map(CborValue::Array),
            (Self::Date, Bson::DateTime(stored)) => {
                let at = chrono::DateTime::<Utc>::from_timestamp_millis(stored.timestamp_millis())?;
                temporal::midnight_date(&temporal::instant(&at)).map(|day| temporal::date(&day))
            }
            (Self::Timestamp, Bson::DateTime(stored)) => {
                let at = chrono::DateTime::<Utc>::from_timestamp_millis(stored.timestamp_millis())?;
                Some(temporal::instant(&at))
            }
            (Self::Date, Bson::String(s)) => {
                date_of(&CborValue::Text(s.clone())).map(|day| temporal::date(&day))
            }
            (Self::Timestamp, Bson::String(s)) => {
                let at = instant_of(&CborValue::Text(s.clone()))?;
                let at = chrono::DateTime::<Utc>::from_timestamp_millis(at.timestamp_millis())?;
                Some(temporal::instant(&at))
            }
            (Self::Time, Bson::String(s)) => {
                temporal::parse_time(s).map(|time| temporal::time(&to_millis(time)))
            }
            _ => None,
        }
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

/// The instant a `timestamp` write names: any instant the `as_instant` reader
/// takes, tag 1 epoch seconds, or a date at its midnight UTC.
fn instant_of(value: &CborValue) -> Option<chrono::DateTime<Utc>> {
    temporal::as_instant(value)
        .or_else(|| epoch_instant(value))
        .or_else(|| temporal::as_date(value).map(|day| day.and_time(NaiveTime::MIN).and_utc()))
}

/// The date a `date` write names: any date the `as_date` reader takes, or an
/// instant (tag 1 epoch seconds included) at exactly midnight UTC.
fn date_of(value: &CborValue) -> Option<NaiveDate> {
    temporal::as_date(value)
        .or_else(|| temporal::midnight_date(value))
        .or_else(|| {
            let at = epoch_instant(value)?;
            (at.time() == NaiveTime::MIN).then(|| at.date_naive())
        })
}

/// Tag 1 epoch seconds.
fn epoch_instant(value: &CborValue) -> Option<chrono::DateTime<Utc>> {
    match value {
        CborValue::Tag(1, inner) => {
            chrono::DateTime::<Utc>::from_timestamp_millis(epoch_millis(inner)?)
        }
        _ => None,
    }
}

fn midnight_millis(day: NaiveDate) -> i64 {
    day.and_time(NaiveTime::MIN).and_utc().timestamp_millis()
}

/// `time` cut to milliseconds, the precision a DateTime keeps.
fn to_millis(time: NaiveTime) -> NaiveTime {
    time.with_nanosecond(time.nanosecond() / 1_000_000 * 1_000_000)
        .unwrap_or(time)
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
        // A `json` array is one value, not a list of values.
        CborValue::Array(items) if kind != BsonKind::Json => items
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
        // Over a stored value, a `json` map or array keeps the stored types
        // of the parts it repeats.
        (Some(old), CborValue::Map(_) | CborValue::Array(_)) if kind == BsonKind::Json => {
            Ok(preserve(old, value))
        }
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
