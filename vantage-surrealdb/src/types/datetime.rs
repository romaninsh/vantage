//! `chrono::DateTime<Utc>` as SurrealDB's compact datetime (CBOR tag 12,
//! `[seconds, nanos]`). Decoding also accepts tag 0 (RFC 3339 text) and plain
//! RFC 3339 text, so records that store datetimes as strings still load.
//!
//! SurrealDB has no date or time-of-day type: a `NaiveDate` is the datetime
//! at midnight UTC, and a `NaiveTime` is text.

use crate::types::{SurrealType, SurrealTypeDateTimeMarker, SurrealTypeStringMarker};
use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Utc};
use ciborium::Value as CborValue;
use vantage_types::cbor_json::{rfc3339_to_tag12, tag12_to_rfc3339};
use vantage_types::temporal;

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
        temporal::as_date(&cbor).or_else(|| temporal::midnight_date(&cbor))
    }
}

/// Text in the carrier form, `HH:MM:SS.ffffff`, so text order is time
/// order; `HH:MM` and a tagged time decode too.
impl SurrealType for NaiveTime {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(temporal::format_time(self))
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        temporal::as_time(&cbor)
    }
}

fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
