//! BSON ↔ CBOR bridge used by the Vista source and the `AnyMongoType`
//! CBOR conversions.
//!
//! Lossy paths (`ObjectId`, `DateTime`, `Decimal128`, `Regex`, `JavaScriptCode`,
//! `Symbol`) collapse to a string representation; `Timestamp` becomes a CBOR
//! map with `t`/`i` fields. `DbPointer` and unknown rare BSON variants surface
//! as `Null`. CBOR `Tag` is unwrapped on the way back (the tag is dropped).

use bson::Bson;
use ciborium::Value as CborValue;

/// Convert a BSON value to its CBOR equivalent.
pub fn bson_to_cbor(value: &Bson) -> CborValue {
    match value {
        Bson::Null | Bson::Undefined | Bson::MaxKey | Bson::MinKey => CborValue::Null,
        Bson::Boolean(b) => CborValue::Bool(*b),
        Bson::Int32(i) => CborValue::Integer((*i).into()),
        Bson::Int64(i) => CborValue::Integer((*i).into()),
        Bson::Double(f) => CborValue::Float(*f),
        Bson::String(s) => CborValue::Text(s.clone()),
        Bson::ObjectId(oid) => CborValue::Text(oid.to_hex()),
        Bson::DateTime(dt) => CborValue::Text(dt.try_to_rfc3339_string().unwrap_or_default()),
        Bson::Binary(bin) => CborValue::Bytes(bin.bytes.clone()),
        Bson::Array(arr) => CborValue::Array(arr.iter().map(bson_to_cbor).collect()),
        Bson::Document(doc) => CborValue::Map(
            doc.iter()
                .map(|(k, v)| (CborValue::Text(k.clone()), bson_to_cbor(v)))
                .collect(),
        ),
        Bson::Decimal128(d) => CborValue::Text(d.to_string()),
        Bson::RegularExpression(r) => CborValue::Map(vec![
            (
                CborValue::Text("$regex".into()),
                CborValue::Text(r.pattern.clone()),
            ),
            (
                CborValue::Text("$options".into()),
                CborValue::Text(r.options.clone()),
            ),
        ]),
        Bson::JavaScriptCode(s) => CborValue::Text(s.clone()),
        Bson::JavaScriptCodeWithScope(c) => CborValue::Text(c.code.clone()),
        Bson::Timestamp(ts) => CborValue::Map(vec![
            (
                CborValue::Text("t".into()),
                CborValue::Integer(ts.time.into()),
            ),
            (
                CborValue::Text("i".into()),
                CborValue::Integer(ts.increment.into()),
            ),
        ]),
        Bson::Symbol(s) => CborValue::Text(s.clone()),
        Bson::DbPointer(_) => CborValue::Null,
    }
}

/// Convert a CBOR value to BSON. Floats coerce to `Double`; integers wider
/// than `i64` stringify rather than silently truncate.
pub fn cbor_to_bson(value: &CborValue) -> Bson {
    match value {
        CborValue::Null => Bson::Null,
        CborValue::Bool(b) => Bson::Boolean(*b),
        CborValue::Integer(i) => {
            if let Ok(v) = i64::try_from(*i) {
                Bson::Int64(v)
            } else {
                Bson::String(format!("{:?}", i))
            }
        }
        CborValue::Float(f) => Bson::Double(*f),
        CborValue::Text(s) => Bson::String(s.clone()),
        CborValue::Bytes(b) => Bson::Binary(bson::Binary {
            subtype: bson::spec::BinarySubtype::Generic,
            bytes: b.clone(),
        }),
        CborValue::Array(arr) => Bson::Array(arr.iter().map(cbor_to_bson).collect()),
        CborValue::Map(entries) => {
            let mut doc = bson::Document::new();
            for (k, v) in entries {
                let key = match k {
                    CborValue::Text(s) => s.clone(),
                    other => format!("{:?}", other),
                };
                doc.insert(key, cbor_to_bson(v));
            }
            Bson::Document(doc)
        }
        CborValue::Tag(_, inner) => cbor_to_bson(inner),
        _ => Bson::Null,
    }
}
