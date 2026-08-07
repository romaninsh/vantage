//! Datetime type implementation for SurrealDB
//!
//! Maps `chrono::DateTime<Utc>` to the SurrealDB compact datetime encoding
//! (CBOR tag 12, `[seconds, nanos]`). The tag-12 conversion itself lives in
//! `vantage_types::cbor_json` and is reused here. Decoding also accepts
//! tag 0 (RFC 3339 text) and plain text, so records that store datetimes as
//! strings still load while a dataset migrates to real datetime values.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_compact() {
        let dt = DateTime::from_timestamp(1_754_000_000, 123_456_789).unwrap();
        let cbor = dt.to_cbor();
        assert!(matches!(cbor, CborValue::Tag(12, _)));
        assert_eq!(DateTime::<Utc>::from_cbor(cbor), Some(dt));
    }

    #[test]
    fn decodes_rfc3339_text() {
        let dt = DateTime::<Utc>::from_cbor(CborValue::Text(
            "2026-03-19T20:25:38.870950016Z".to_string(),
        ))
        .unwrap();
        assert_eq!(dt.timestamp_subsec_nanos(), 870_950_016);
    }

    #[test]
    fn rejects_other_shapes() {
        assert_eq!(
            DateTime::<Utc>::from_cbor(CborValue::Integer(5.into())),
            None
        );
        assert_eq!(
            DateTime::<Utc>::from_cbor(CborValue::Text("not a date".into())),
            None
        );
    }
}
