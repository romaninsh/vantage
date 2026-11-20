//! The carrier forms of dates, times and instants.
//!
//! Every driver emits these and nothing else: an instant is `Tag(0)` RFC 3339 text in UTC with
//! six fraction digits and `Z`, a date is `Tag(1004)` full-date text, a time of day is `Tag(101)`
//! text with six fraction digits. Fixed width keeps text order equal to time order wherever a
//! value is compared as text. No naive value travels between layers: a driver reading naive
//! storage treats it as UTC.
//!
//! The `as_*` readers are lenient, for input from forms, scripts and old fixtures: they also
//! accept offsets, naive text, `HH:MM`, the legacy `Tag(100)` date and SurrealDB's `Tag(12)`
//! epoch pair. Only the canonical forms go out.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone as _, Utc};
use ciborium::Value as CborValue;

/// RFC 8949 standard date/time string.
pub const TAG_INSTANT: u64 = 0;
/// RFC 8943 full-date string.
pub const TAG_DATE: u64 = 1004;
/// Time of day; CBOR registers no tag for it.
pub const TAG_TIME: u64 = 101;
/// Accepted on input only: the date tag drivers emitted before `TAG_DATE`. Its text form isn't
/// RFC 8943's tag 100 (integer days since 1970), which is read too.
pub const TAG_LEGACY_DATE: u64 = 100;
/// Accepted on input only: SurrealDB's `[seconds, nanos]` datetime.
pub const TAG_SURREAL_INSTANT: u64 = 12;

const INSTANT_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.6fZ";
const DATE_FORMAT: &str = "%Y-%m-%d";
const TIME_FORMAT: &str = "%H:%M:%S%.6f";

pub fn format_instant(at: &DateTime<Utc>) -> String {
    at.format(INSTANT_FORMAT).to_string()
}

pub fn format_date(day: &NaiveDate) -> String {
    day.format(DATE_FORMAT).to_string()
}

pub fn format_time(time: &NaiveTime) -> String {
    time.format(TIME_FORMAT).to_string()
}

pub fn instant(at: &DateTime<Utc>) -> CborValue {
    tagged(TAG_INSTANT, format_instant(at))
}

pub fn date(day: &NaiveDate) -> CborValue {
    tagged(TAG_DATE, format_date(day))
}

pub fn time(time: &NaiveTime) -> CborValue {
    tagged(TAG_TIME, format_time(time))
}

/// RFC 3339 with any offset, or naive text (`T` or a space, any fraction) taken as UTC.
pub fn parse_instant(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(at) = DateTime::parse_from_rfc3339(s) {
        return Some(at.with_timezone(&Utc));
    }
    // Postgres renders a zone-aware value with a short offset (`+00`), which RFC 3339 refuses.
    if let Some(at) = ["%Y-%m-%d %H:%M:%S%.f%#z", "%Y-%m-%dT%H:%M:%S%.f%#z"]
        .iter()
        .find_map(|f| DateTime::parse_from_str(s, f).ok())
    {
        return Some(at.with_timezone(&Utc));
    }
    [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ]
    .iter()
    .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
    .map(|naive| naive.and_utc())
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), DATE_FORMAT).ok()
}

/// `HH:MM`, `HH:MM:SS` or `HH:MM:SS.f`.
pub fn parse_time(s: &str) -> Option<NaiveTime> {
    let s = s.trim();
    NaiveTime::parse_from_str(s, "%H:%M:%S%.f")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M"))
        .ok()
}

/// An instant from any accepted carrier form. A date isn't one: widening it to midnight would
/// hide a value stored in the wrong kind of column (the lenient write path proposes the midnight
/// instant itself where a date may stand for one).
pub fn as_instant(value: &CborValue) -> Option<DateTime<Utc>> {
    match value {
        CborValue::Tag(TAG_SURREAL_INSTANT, inner) => epoch_pair(inner),
        CborValue::Tag(TAG_DATE | TAG_LEGACY_DATE | TAG_TIME, _) => None,
        CborValue::Tag(_, inner) => as_instant(inner),
        CborValue::Text(s) => parse_instant(s),
        _ => None,
    }
}

/// A date from any accepted carrier form. An instant isn't one, even at midnight: a driver that
/// stores dates as midnight instants converts them itself, knowing the column.
pub fn as_date(value: &CborValue) -> Option<NaiveDate> {
    match value {
        CborValue::Tag(TAG_DATE | TAG_LEGACY_DATE, inner) => match &**inner {
            CborValue::Text(s) => parse_date(s),
            // RFC 8943's tag 100: days since 1970-01-01.
            CborValue::Integer(days) => {
                let days = i64::try_from(i128::from(*days)).ok()?;
                NaiveDate::from_ymd_opt(1970, 1, 1)?
                    .checked_add_signed(chrono::Duration::days(days))
            }
            _ => None,
        },
        CborValue::Tag(TAG_INSTANT | TAG_SURREAL_INSTANT | TAG_TIME, _) => None,
        CborValue::Tag(_, inner) => as_date(inner),
        CborValue::Text(s) => parse_date(s),
        _ => None,
    }
}

/// The date of an instant at exactly midnight UTC: for a driver that stores dates that way. A
/// later time would make the date depend on a time zone.
pub fn midnight_date(value: &CborValue) -> Option<NaiveDate> {
    let at = as_instant(value)?;
    (at.time() == NaiveTime::MIN).then(|| at.date_naive())
}

pub fn as_time(value: &CborValue) -> Option<NaiveTime> {
    match value {
        CborValue::Tag(_, inner) => as_time(inner),
        CborValue::Text(s) => parse_time(s),
        _ => None,
    }
}

fn epoch_pair(inner: &CborValue) -> Option<DateTime<Utc>> {
    let CborValue::Array(parts) = inner else {
        return None;
    };
    let int = |v: &CborValue| match v {
        CborValue::Integer(i) => i64::try_from(i128::from(*i)).ok(),
        _ => None,
    };
    let secs = int(parts.first()?)?;
    let nanos = match parts.get(1) {
        Some(v) => u32::try_from(int(v)?).ok()?,
        None => 0,
    };
    if parts.len() > 2 {
        return None;
    }
    Utc.timestamp_opt(secs, nanos).single()
}

fn tagged(tag: u64, text: String) -> CborValue {
    CborValue::Tag(tag, Box::new(CborValue::Text(text)))
}
