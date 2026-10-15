//! Variant-preserving CBOR encoding for `Record<AnyRedbType>` row bodies.
//!
//! On disk a row is a CBOR array of triples `[name, variant_index_or_null, value]`.
//! The variant index is the `RedbTypeVariants` discriminant from
//! `RedbTypeVariants::to_index`; a CBOR `Null` means the original write was
//! untyped, in which case the value gets re-tagged from its CBOR shape on read.
//!
//! Index keys are encoded the same way (single value, no name/variant
//! wrapper) via `value_to_index_key` so range scans on the index table
//! line up with values produced by `encode_value`.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::Record;

use super::{AnyRedbType, RedbTypeVariants};

/// Encode a record into bytes for storage in the main table.
pub fn encode_record(record: &Record<AnyRedbType>) -> Result<Vec<u8>> {
    let triples: Vec<CborValue> = record
        .iter()
        .map(|(name, val)| {
            let variant = match val.type_variant() {
                Some(v) => CborValue::Integer((v.to_index() as i64).into()),
                None => CborValue::Null,
            };
            CborValue::Array(vec![
                CborValue::Text(name.clone()),
                variant,
                val.value().clone(),
            ])
        })
        .collect();

    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&CborValue::Array(triples), &mut bytes)
        .map_err(|e| error!("CBOR encode failed", details = e.to_string()))?;
    Ok(bytes)
}

/// Decode bytes from the main table back into a record.
pub fn decode_record(bytes: &[u8]) -> Result<Record<AnyRedbType>> {
    let parsed: CborValue = ciborium::de::from_reader(bytes)
        .map_err(|e| error!("CBOR decode failed", details = e.to_string()))?;
    let triples = match parsed {
        CborValue::Array(items) => items,
        _ => return Err(error!("Expected CBOR array at row body")),
    };

    let mut record: Record<AnyRedbType> = Record::new();
    for triple in triples {
        let parts = match triple {
            CborValue::Array(p) if p.len() == 3 => p,
            _ => return Err(error!("Row body triple shape mismatch")),
        };
        let mut iter = parts.into_iter();
        let name = match iter.next() {
            Some(CborValue::Text(s)) => s,
            _ => return Err(error!("Row body field name must be text")),
        };
        let variant = match iter.next() {
            Some(CborValue::Null) => None,
            Some(CborValue::Integer(i)) => i64::try_from(i)
                .ok()
                .and_then(|n| u8::try_from(n).ok())
                .and_then(RedbTypeVariants::from_index),
            _ => return Err(error!("Row body variant tag must be integer or null")),
        };
        let value = iter.next().unwrap_or(CborValue::Null);

        // If the write was untyped, re-tag by inspecting the CBOR shape.
        let variant = variant.or_else(|| RedbTypeVariants::from_cbor(&value));

        record.insert(name, AnyRedbType::untyped_with(value, variant));
    }
    Ok(record)
}

/// Encode a single value (used to build index keys). Variant tag is **not**
/// stored in the index — only the raw CBOR value, so that two values that
/// compare equal at the value level produce identical key bytes.
pub fn encode_value(value: &CborValue) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes)
        .map_err(|e| error!("CBOR value encode failed", details = e.to_string()))?;
    Ok(bytes)
}

/// Build the byte key used in an index table for a given `AnyRedbType` value.
pub fn value_to_index_key(value: &AnyRedbType) -> Result<Vec<u8>> {
    encode_value(value.value())
}

// The type-system macro has no constructor taking both a value and an
// explicit variant; the on-disk tag can disagree with the CBOR shape.
impl AnyRedbType {
    pub(crate) fn untyped_with(value: CborValue, variant: Option<RedbTypeVariants>) -> Self {
        Self {
            value,
            type_variant: variant,
        }
    }
}
