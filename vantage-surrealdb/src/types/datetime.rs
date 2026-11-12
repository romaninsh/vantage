//! `chrono::DateTime<Utc>` as SurrealDB's compact datetime (CBOR tag 12,
//! `[seconds, nanos]`). Decoding also accepts tag 0 (RFC 3339 text) and plain
//! RFC 3339 text, so records that store datetimes as strings still load.
//!
//! SurrealDB has no date or time-of-day type: a `NaiveDate` is the datetime
//! at midnight UTC, and a `NaiveTime` is text.

use crate::types::{SurrealType, SurrealTypeDateTimeMarker, SurrealTypeStringMarker};
use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Timelike, Utc};
use ciborium::Value as CborValue;
use vantage_types::cbor_json::{rfc3339_to_tag12, tag12_to_rfc3339};

// Date and time-of-day text tags (vantage-types' lenient forms, RFC 8943).
const TAG_DATE: u64 = 100;
const TAG_DATE_TEXT: u64 = 1004;
const TAG_TIME: u64 = 101;

impl SurrealType for DateTime<Utc> {
    type Target = SurrealTypeDateTimeMarker;

    fn to_cbor(&self) -> CborValue {
        rfc3339_to_tag12(&self.to_rfc3339_opts(SecondsFormat::Nanos, true))
            .expect("RFC 3339 text from DateTime<Utc> always converts")
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(12, inner) => parse_rfc3339(&tag12_to_rfc3339(&inner)?),
            CborValue::Tag(0, inner) => match *inner {
                CborValue::Text(s) => parse_rfc3339(&s),
                _ => None,
            },
            CborValue::Text(s) => parse_rfc3339(&s),
            _ => None,
        }
    }
}

/// Stored as the datetime at midnight UTC, the way SurrealDB reads it back.
/// A datetime at any other time is not a date: truncating it would depend
/// on the timezone.
impl SurrealType for NaiveDate {
    type Target = SurrealTypeDateTimeMarker;

    fn to_cbor(&self) -> CborValue {
        self.and_time(NaiveTime::MIN).and_utc().to_cbor()
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(TAG_DATE | TAG_DATE_TEXT, inner) => match *inner {
                CborValue::Text(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok(),
                _ => None,
            },
            tag12 @ CborValue::Tag(12, _) => {
                let instant = DateTime::<Utc>::from_cbor(tag12)?;
                (instant.time() == NaiveTime::MIN).then(|| instant.date_naive())
            }
            _ => None,
        }
    }
}

/// Text, `HH:MM:SS` with a fraction when there is one; `HH:MM` decodes too.
impl SurrealType for NaiveTime {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        let format = if self.nanosecond() == 0 {
            "%H:%M:%S"
        } else {
            "%H:%M:%S%.f"
        };
        CborValue::Text(self.format(format).to_string())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        let text = match cbor {
            CborValue::Text(s) => s,
            CborValue::Tag(TAG_TIME, inner) => match *inner {
                CborValue::Text(s) => s,
                _ => return None,
            },
            _ => return None,
        };
        NaiveTime::parse_from_str(&text, "%H:%M:%S%.f")
            .or_else(|_| NaiveTime::parse_from_str(&text, "%H:%M"))
            .ok()
    }
}

fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
