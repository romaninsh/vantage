//! `chrono::DateTime<Utc>` as SurrealDB's compact datetime (CBOR tag 12,
//! `[seconds, nanos]`). Decoding also accepts tag 0 (RFC 3339 text) and plain
//! RFC 3339 text, so records that store datetimes as strings still load.

use crate::types::{SurrealType, SurrealTypeDateTimeMarker};
use chrono::{DateTime, SecondsFormat, Utc};
use ciborium::Value as CborValue;
use vantage_types::cbor_json::{rfc3339_to_tag12, tag12_to_rfc3339};

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

fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
