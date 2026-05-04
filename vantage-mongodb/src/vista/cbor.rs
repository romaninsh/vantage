//! BSON ↔ CBOR bridge used by the Vista source.
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

#[cfg(test)]
mod tests {
    use super::*;
    use bson::oid::ObjectId;

    #[test]
    fn bson_scalars_round_trip_to_cbor() {
        assert_eq!(bson_to_cbor(&Bson::Null), CborValue::Null);
        assert_eq!(bson_to_cbor(&Bson::Boolean(true)), CborValue::Bool(true));
        assert_eq!(
            bson_to_cbor(&Bson::Int32(42)),
            CborValue::Integer(42i64.into())
        );
        assert_eq!(
            bson_to_cbor(&Bson::Int64(-7)),
            CborValue::Integer((-7i64).into())
        );
        assert_eq!(bson_to_cbor(&Bson::Double(2.5)), CborValue::Float(2.5));
        assert_eq!(
            bson_to_cbor(&Bson::String("hi".into())),
            CborValue::Text("hi".into())
        );
    }

    #[test]
    fn bson_objectid_stringifies_in_cbor() {
        let oid = ObjectId::new();
        let cbor = bson_to_cbor(&Bson::ObjectId(oid));
        assert_eq!(cbor, CborValue::Text(oid.to_hex()));
    }

    #[test]
    fn bson_document_becomes_cbor_map() {
        let mut doc = bson::Document::new();
        doc.insert("name", "Marty");
        doc.insert("age", 17_i64);
        let cbor = bson_to_cbor(&Bson::Document(doc));
        let CborValue::Map(entries) = cbor else {
            panic!("expected map");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, CborValue::Text("name".into()));
        assert_eq!(entries[0].1, CborValue::Text("Marty".into()));
        assert_eq!(entries[1].0, CborValue::Text("age".into()));
        assert_eq!(entries[1].1, CborValue::Integer(17i64.into()));
    }

    #[test]
    fn bson_array_becomes_cbor_array() {
        let arr = Bson::Array(vec![Bson::Int64(1), Bson::Int64(2), Bson::Int64(3)]);
        let cbor = bson_to_cbor(&arr);
        assert_eq!(
            cbor,
            CborValue::Array(vec![
                CborValue::Integer(1i64.into()),
                CborValue::Integer(2i64.into()),
                CborValue::Integer(3i64.into()),
            ])
        );
    }

    #[test]
    fn cbor_scalars_round_trip_to_bson() {
        assert_eq!(cbor_to_bson(&CborValue::Null), Bson::Null);
        assert_eq!(cbor_to_bson(&CborValue::Bool(false)), Bson::Boolean(false));
        assert_eq!(
            cbor_to_bson(&CborValue::Integer(99i64.into())),
            Bson::Int64(99)
        );
        assert_eq!(cbor_to_bson(&CborValue::Float(1.5)), Bson::Double(1.5));
        assert_eq!(
            cbor_to_bson(&CborValue::Text("x".into())),
            Bson::String("x".into())
        );
    }

    #[test]
    fn cbor_map_becomes_bson_document() {
        let cbor = CborValue::Map(vec![
            (CborValue::Text("k".into()), CborValue::Bool(true)),
            (CborValue::Text("n".into()), CborValue::Integer(7i64.into())),
        ]);
        let Bson::Document(doc) = cbor_to_bson(&cbor) else {
            panic!("expected doc");
        };
        assert!(doc.get_bool("k").unwrap());
        assert_eq!(doc.get_i64("n").unwrap(), 7);
    }

    #[test]
    fn round_trip_through_cbor_preserves_basic_values() {
        let mut doc = bson::Document::new();
        doc.insert("name", "Doc");
        doc.insert("year", 1985_i64);
        doc.insert("active", true);
        let original = Bson::Document(doc);

        let cbor = bson_to_cbor(&original);
        let back = cbor_to_bson(&cbor);

        let Bson::Document(d) = back else {
            panic!("expected doc");
        };
        assert_eq!(d.get_str("name").unwrap(), "Doc");
        assert_eq!(d.get_i64("year").unwrap(), 1985);
        assert!(d.get_bool("active").unwrap());
    }
}
