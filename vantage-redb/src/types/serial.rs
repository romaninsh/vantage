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

        record.insert(
            name,
            AnyRedbType::untyped_with(value, variant),
        );
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

// --- private constructor on AnyRedbType --------------------------------
//
// We need a way to construct AnyRedbType with both a value AND an explicit
// variant (sometimes the variant comes from the on-disk tag, not from the
// CBOR shape). The macro doesn't expose this directly; we add it here as a
// sibling-module method (private fields are accessible because we're in the
// same `types` module tree).

impl AnyRedbType {
    pub(crate) fn untyped_with(value: CborValue, variant: Option<RedbTypeVariants>) -> Self {
        Self {
            value,
            type_variant: variant,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip_typed() {
        let mut rec: Record<AnyRedbType> = Record::new();
        rec.insert("name".into(), AnyRedbType::new("Alice".to_string()));
        rec.insert("age".into(), AnyRedbType::new(30i64));
        rec.insert("active".into(), AnyRedbType::new(true));

        let bytes = encode_record(&rec).unwrap();
        let back = decode_record(&bytes).unwrap();

        assert_eq!(back.len(), 3);
        assert_eq!(
            back["name"].try_get::<String>(),
            Some("Alice".to_string())
        );
        assert_eq!(back["age"].try_get::<i64>(), Some(30));
        assert_eq!(back["active"].try_get::<bool>(), Some(true));

        // Variant tags survive round-trip.
        assert_eq!(
            back["name"].type_variant(),
            Some(RedbTypeVariants::String)
        );
        assert_eq!(back["age"].type_variant(), Some(RedbTypeVariants::Int));
        assert_eq!(
            back["active"].type_variant(),
            Some(RedbTypeVariants::Bool)
        );
    }

    #[test]
    fn test_round_trip_untyped_retagged_from_cbor() {
        let mut rec: Record<AnyRedbType> = Record::new();
        rec.insert(
            "x".into(),
            AnyRedbType::untyped(CborValue::Integer(7i64.into())),
        );

        let bytes = encode_record(&rec).unwrap();
        let back = decode_record(&bytes).unwrap();

        // No on-disk variant → re-tagged from CBOR shape (Integer → Int).
        assert_eq!(back["x"].type_variant(), Some(RedbTypeVariants::Int));
        assert_eq!(back["x"].try_get::<i64>(), Some(7));
    }

    #[test]
    fn test_index_key_matches_value() {
        let v1 = AnyRedbType::new(42i64);
        let v2 = AnyRedbType::untyped(CborValue::Integer(42i64.into()));
        // typed and untyped same value should hash identically — index lookups
        // must match regardless of variant tag.
        assert_eq!(
            value_to_index_key(&v1).unwrap(),
            value_to_index_key(&v2).unwrap()
        );
    }
}
