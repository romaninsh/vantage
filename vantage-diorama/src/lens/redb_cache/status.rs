//! Stored row layout: a one-byte [`CacheStatus`] tag followed by the
//! CBOR-encoded record map.
//!
//! The tag bytes (0/1) are distinct from a CBOR map's leading byte (major
//! type 5, 0xA0..=0xBF), so a raw-CBOR row written without a tag is detected
//! on read and treated as `Complete`.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::lens::cache_backend::CacheStatus;

const TAG_COMPLETE: u8 = 0;
const TAG_INCOMPLETE: u8 = 1;

fn status_tag(status: CacheStatus) -> u8 {
    match status {
        CacheStatus::Complete => TAG_COMPLETE,
        CacheStatus::Incomplete => TAG_INCOMPLETE,
    }
}

pub(super) fn encode(record: &Record<CborValue>, status: CacheStatus) -> Result<Vec<u8>> {
    let map: Vec<(CborValue, CborValue)> = record
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), v.clone()))
        .collect();
    let value = CborValue::Map(map);
    let mut buf = Vec::with_capacity(64);
    buf.push(status_tag(status));
    ciborium::into_writer(&value, &mut buf)
        .map_err(|e| error!("Failed to encode cache row", detail = e.to_string()))?;
    Ok(buf)
}

pub(super) fn decode(bytes: &[u8]) -> Result<(Record<CborValue>, CacheStatus)> {
    let (status, body) = match bytes.first() {
        Some(&TAG_COMPLETE) => (CacheStatus::Complete, &bytes[1..]),
        Some(&TAG_INCOMPLETE) => (CacheStatus::Incomplete, &bytes[1..]),
        // Untagged row (raw CBOR map) — treat as complete.
        _ => (CacheStatus::Complete, bytes),
    };
    let value: CborValue = ciborium::from_reader(body)
        .map_err(|e| error!("Failed to decode cache row", detail = e.to_string()))?;
    match value {
        CborValue::Map(entries) => {
            let mut record = Record::new();
            for (k, v) in entries {
                let CborValue::Text(key) = k else {
                    return Err(error!("cache row had non-text key"));
                };
                record.insert(key, v);
            }
            Ok((record, status))
        }
        other => Err(error!(
            "cache row not a cbor map",
            shape = format!("{:?}", other)
        )),
    }
}
