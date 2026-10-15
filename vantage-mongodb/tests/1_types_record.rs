//! Test 1b: Record<AnyMongoType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.

use std::collections::HashSet;
use std::str::FromStr;

use bson::oid::ObjectId;
use bson::spec::BinarySubtype;
use bson::{Binary, Bson, DateTime, Decimal128};
use ciborium::Value as CborValue;
use vantage_mongodb::types::cbor::{bson_to_cbor, cbor_to_bson};
use vantage_mongodb::{AnyMongoType, MongoId};
use vantage_types::Record;

// ── Typed records (write path) ─────────────────────────────────────────────
// Values created with AnyMongoType::new() carry variant tags.

#[test]
fn test_typed_record_creation() {
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert("name".into(), AnyMongoType::new("Cupcake".to_string()));
    record.insert("price".into(), AnyMongoType::new(120i64));
    record.insert("is_deleted".into(), AnyMongoType::new(false));

    assert_eq!(
        record["name"].try_get::<String>(),
        Some("Cupcake".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["is_deleted"].try_get::<bool>(), Some(false));

    // Type markers enforce boundaries — wrong type → None
    assert_eq!(record["name"].try_get::<i64>(), None);
    assert_eq!(record["price"].try_get::<String>(), None);
}

#[test]
fn test_typed_record_with_option() {
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert(
        "nickname".into(),
        AnyMongoType::new(Some("Ali".to_string())),
    );
    record.insert("note".into(), AnyMongoType::new(None::<String>));

    assert_eq!(
        record["nickname"].try_get::<Option<String>>(),
        Some(Some("Ali".to_string()))
    );
    assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
}

#[test]
fn test_typed_record_i32_vs_i64() {
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert("small".into(), AnyMongoType::new(42i32));
    record.insert("big".into(), AnyMongoType::new(42i64));

    // i32 → Int32, i64 → Int64: different variants
    assert_eq!(record["small"].try_get::<i32>(), Some(42));
    assert_eq!(record["big"].try_get::<i64>(), Some(42));

    // Cross-variant blocked
    assert_eq!(record["small"].try_get::<i64>(), None); // Int32 ≠ Int64
    assert_eq!(record["big"].try_get::<i32>(), None); // Int64 ≠ Int32
}

// ── Untyped records (read path) ────────────────────────────────────────────
// Values created with AnyMongoType::untyped() have type_variant: None.
// try_get is permissive — it attempts conversion without variant check.

#[test]
fn test_untyped_record_creation() {
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert(
        "name".into(),
        AnyMongoType::untyped(Bson::String("Cupcake".into())),
    );
    record.insert("price".into(), AnyMongoType::untyped(Bson::Int64(120)));
    record.insert("active".into(), AnyMongoType::untyped(Bson::Boolean(true)));

    assert_eq!(
        record["name"].try_get::<String>(),
        Some("Cupcake".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));

    // Still fails when underlying value can't convert
    assert_eq!(record["name"].try_get::<i64>(), None);
    assert_eq!(record["price"].try_get::<String>(), None);
}

#[test]
fn test_untyped_null() {
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert("note".into(), AnyMongoType::untyped(Bson::Null));

    assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
    assert_eq!(record["note"].try_get::<Option<i64>>(), Some(None));
}

#[test]
fn test_untyped_int64_as_i32_permissive() {
    // Untyped: no variant check, Bson::Int64(42) can convert to i32 via from_bson
    let val = AnyMongoType::untyped(Bson::Int64(42));
    assert_eq!(val.try_get::<i64>(), Some(42));
    assert_eq!(val.try_get::<i32>(), Some(42)); // permissive — from_bson accepts Int64 → i32
}

#[test]
fn test_untyped_int32_as_i64_permissive() {
    let val = AnyMongoType::untyped(Bson::Int32(42));
    assert_eq!(val.try_get::<i32>(), Some(42));
    assert_eq!(val.try_get::<i64>(), Some(42)); // from_bson accepts Int32 → i64
}

#[test]
fn test_untyped_int_as_f64_permissive() {
    // Bson::Int64(42) can convert to f64 via from_bson
    let val = AnyMongoType::untyped(Bson::Int64(42));
    assert_eq!(val.try_get::<f64>(), Some(42.0));
}

// ── Typed vs untyped comparison ────────────────────────────────────────────

#[test]
fn test_typed_blocks_cross_variant() {
    let typed = AnyMongoType::new(42i64);
    assert_eq!(typed.try_get::<i64>(), Some(42));
    assert_eq!(typed.try_get::<f64>(), None); // Int64 ≠ Double → blocked
    assert_eq!(typed.try_get::<i32>(), None); // Int64 ≠ Int32 → blocked

    let untyped = AnyMongoType::untyped(Bson::Int64(42));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
    assert_eq!(untyped.try_get::<f64>(), Some(42.0)); // permissive
    assert_eq!(untyped.try_get::<i32>(), Some(42)); // permissive
}

// ── MongoDB-specific: bool is native ───────────────────────────────────────

#[test]
fn test_typed_bool_in_record() {
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert("active".into(), AnyMongoType::new(true));

    assert_eq!(*record["active"].value(), Bson::Boolean(true));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));
    // Bool ≠ Int64 → blocked
    assert_eq!(record["active"].try_get::<i64>(), None);
}

// ── MongoDB-specific: ObjectId in records ──────────────────────────────────

#[test]
fn test_objectid_in_record() {
    let oid = bson::oid::ObjectId::new();
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert("_id".into(), AnyMongoType::new(oid));

    assert_eq!(record["_id"].try_get::<bson::oid::ObjectId>(), Some(oid));
    // ObjectId ≠ String → blocked for typed
    assert_eq!(record["_id"].try_get::<String>(), None);
}

#[test]
fn test_untyped_objectid_from_string() {
    // When reading from DB, ObjectId comes back as Bson::ObjectId
    let oid = bson::oid::ObjectId::new();
    let val = AnyMongoType::untyped(Bson::ObjectId(oid));
    assert_eq!(val.try_get::<bson::oid::ObjectId>(), Some(oid));
}

#[test]
fn test_untyped_string_as_objectid() {
    // A hex string stored as Bson::String can parse into ObjectId via from_bson
    let oid = bson::oid::ObjectId::new();
    let val = AnyMongoType::untyped(Bson::String(oid.to_hex()));
    assert_eq!(val.try_get::<bson::oid::ObjectId>(), Some(oid));
}

// ── Error cases ────────────────────────────────────────────────────────────

#[test]
fn test_missing_field_in_record() {
    let record: Record<AnyMongoType> = Record::new();
    assert!(record.get("name").is_none());
}

#[test]
fn test_typed_wrong_extraction() {
    let mut record: Record<AnyMongoType> = Record::new();
    record.insert("name".into(), AnyMongoType::new(42i64));
    assert_eq!(record["name"].try_get::<String>(), None);
}

// ── TryFrom<AnyMongoType> for Record<AnyMongoType> ────────────────────────

#[test]
fn test_try_from_document() {
    let doc = bson::doc! {
        "name": "Cupcake",
        "price": 120_i64,
        "active": true
    };
    let any = AnyMongoType::untyped(Bson::Document(doc));
    let record: Record<AnyMongoType> = any.try_into().unwrap();

    assert_eq!(record["name"].try_get::<String>(), Some("Cupcake".into()));
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));
}

#[test]
fn test_try_from_array_extracts_first() {
    let doc1 = bson::doc! { "name": "First" };
    let doc2 = bson::doc! { "name": "Second" };
    let arr = Bson::Array(vec![Bson::Document(doc1), Bson::Document(doc2)]);
    let any = AnyMongoType::untyped(arr);
    let record: Record<AnyMongoType> = any.try_into().unwrap();

    // Should extract the first document
    assert_eq!(record["name"].try_get::<String>(), Some("First".into()));
}

#[test]
fn test_try_from_non_document_fails() {
    let any = AnyMongoType::untyped(Bson::String("not a doc".into()));
    let result: Result<Record<AnyMongoType>, _> = any.try_into();
    assert!(result.is_err());
}

// ── Numbers ────────────────────────────────────────────────────────────────

#[test]
fn test_typed_f64_is_a_bson_double() {
    let val = AnyMongoType::new(2.5f64);
    assert_eq!(*val.value(), Bson::Double(2.5));
    assert_eq!(val.try_get::<f64>(), Some(2.5));
    assert_eq!(val.try_get::<i64>(), None);
}

#[test]
fn test_untyped_integers_read_as_f64() {
    assert_eq!(
        AnyMongoType::untyped(Bson::Int32(3)).try_get::<f64>(),
        Some(3.0)
    );
    assert_eq!(
        AnyMongoType::untyped(Bson::Double(0.25)).try_get::<f64>(),
        Some(0.25)
    );
}

#[test]
fn test_untyped_int64_too_wide_for_i32_is_refused() {
    let val = AnyMongoType::untyped(Bson::Int64(i64::from(i32::MAX) + 1));
    assert_eq!(val.try_get::<i32>(), None);
    assert_eq!(val.try_get::<i64>(), Some(i64::from(i32::MAX) + 1));
}

// ── MongoId ────────────────────────────────────────────────────────────────

#[test]
fn mongo_id_from_bson_takes_object_ids_and_strings_only() {
    let oid = ObjectId::new();
    assert_eq!(
        MongoId::from_bson(&Bson::ObjectId(oid)),
        Some(MongoId::ObjectId(oid))
    );
    assert_eq!(
        MongoId::from_bson(&Bson::String("flux_cupcake".into())),
        Some(MongoId::String("flux_cupcake".into()))
    );
    assert_eq!(MongoId::from_bson(&Bson::Int32(42)), None);
}

#[test]
fn mongo_id_to_bson_keeps_its_variant() {
    let oid = ObjectId::new();
    assert_eq!(MongoId::ObjectId(oid).to_bson(), Bson::ObjectId(oid));
    assert_eq!(
        MongoId::String("abc".into()).to_bson(),
        Bson::String("abc".into())
    );
    assert_eq!(Bson::from(MongoId::ObjectId(oid)), Bson::ObjectId(oid));
}

#[test]
fn mongo_id_displays_as_hex_or_the_raw_string() {
    let oid = ObjectId::new();
    assert_eq!(MongoId::ObjectId(oid).to_string(), oid.to_hex());
    assert_eq!(MongoId::String("hello".into()).to_string(), "hello");
}

#[test]
fn mongo_id_parses_24_char_hex_as_an_object_id_and_anything_else_as_text() {
    let oid = ObjectId::new();
    assert_eq!(
        MongoId::from_str(&oid.to_hex()).unwrap(),
        MongoId::ObjectId(oid)
    );
    assert_eq!(
        MongoId::from_str("flux_cupcake").unwrap(),
        MongoId::String("flux_cupcake".into())
    );
}

#[test]
fn mongo_id_from_conversions_follow_the_same_hex_rule() {
    let oid = ObjectId::new();
    assert_eq!(MongoId::from(oid), MongoId::ObjectId(oid));
    assert_eq!(MongoId::from(oid.to_hex()), MongoId::ObjectId(oid));
    assert_eq!(MongoId::from(oid.to_hex().as_str()), MongoId::ObjectId(oid));
    assert_eq!(MongoId::from("hello"), MongoId::String("hello".into()));
    assert_eq!(
        MongoId::from(String::from("world")),
        MongoId::String("world".into())
    );
    // Right length, not hex: stays text.
    let near_miss = "z".repeat(24);
    assert_eq!(
        MongoId::from(near_miss.as_str()),
        MongoId::String(near_miss)
    );
}

#[test]
fn mongo_id_variants_with_the_same_hex_are_different_keys() {
    let oid = ObjectId::new();
    let mut set = HashSet::new();
    set.insert(MongoId::ObjectId(oid));
    set.insert(MongoId::String(oid.to_hex()));
    set.insert(MongoId::String("abc".into()));
    set.insert(MongoId::String("abc".into()));
    assert_eq!(set.len(), 3);
}

#[test]
fn mongo_id_serializes_as_a_plain_string() {
    let oid = ObjectId::new();
    assert_eq!(
        serde_json::to_string(&MongoId::ObjectId(oid)).unwrap(),
        format!("\"{}\"", oid.to_hex())
    );
    assert_eq!(
        serde_json::to_string(&MongoId::String("abc".into())).unwrap(),
        "\"abc\""
    );
}

// ── Conversions into AnyMongoType ──────────────────────────────────────────

#[test]
fn a_24_char_hex_str_converts_to_an_object_id_and_other_text_stays_a_string() {
    let oid = ObjectId::new();
    let id = AnyMongoType::from(oid.to_hex().as_str());
    assert_eq!(*id.value(), Bson::ObjectId(oid));
    assert_eq!(id.try_get::<ObjectId>(), Some(oid));

    let text = AnyMongoType::from("plain");
    assert_eq!(*text.value(), Bson::String("plain".into()));
    assert_eq!(text.try_get::<String>(), Some("plain".to_string()));

    let near_miss = "z".repeat(24);
    assert_eq!(
        *AnyMongoType::from(near_miss.as_str()).value(),
        Bson::String(near_miss)
    );
}

#[test]
fn a_mongo_id_converts_to_a_value_of_its_own_kind() {
    let oid = ObjectId::new();
    assert_eq!(
        *AnyMongoType::from(MongoId::ObjectId(oid)).value(),
        Bson::ObjectId(oid)
    );
    assert_eq!(
        *AnyMongoType::from(MongoId::String("abc".into())).value(),
        Bson::String("abc".into())
    );
}

#[test]
fn json_converts_into_a_value_and_malformed_extended_json_does_not_panic() {
    let from_json = AnyMongoType::from(serde_json::json!({ "name": "Marty", "tags": ["a"] }));
    assert_eq!(
        *from_json.value(),
        Bson::Document(bson::doc! { "name": "Marty", "tags": ["a"] })
    );

    let malformed = AnyMongoType::from(serde_json::json!({ "$oid": "not-a-hex-id" }));
    assert!(matches!(malformed.value(), Bson::Document(_)));
}

#[test]
fn a_value_converts_to_json_and_to_cbor() {
    let text = AnyMongoType::untyped(Bson::String("hi".into()));
    assert_eq!(
        serde_json::Value::from(text.clone()),
        serde_json::json!("hi")
    );
    assert_eq!(ciborium::Value::from(text), CborValue::Text("hi".into()));

    let oid = ObjectId::new();
    let id = AnyMongoType::untyped(Bson::ObjectId(oid));
    assert_eq!(ciborium::Value::from(id), CborValue::Text(oid.to_hex()));

    assert_eq!(
        *AnyMongoType::from(CborValue::Bool(true)).value(),
        Bson::Boolean(true)
    );
}

// ── BSON <-> CBOR bridge ───────────────────────────────────────────────────

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

#[test]
fn bson_scalars_become_their_cbor_counterparts() {
    assert_eq!(bson_to_cbor(&Bson::Null), CborValue::Null);
    assert_eq!(bson_to_cbor(&Bson::Boolean(true)), CborValue::Bool(true));
    assert_eq!(bson_to_cbor(&Bson::Int32(42)), int(42));
    assert_eq!(bson_to_cbor(&Bson::Int64(-7)), int(-7));
    assert_eq!(bson_to_cbor(&Bson::Double(2.5)), CborValue::Float(2.5));
    assert_eq!(
        bson_to_cbor(&Bson::String("hi".into())),
        CborValue::Text("hi".into())
    );
    assert_eq!(
        bson_to_cbor(&Bson::Binary(Binary {
            subtype: BinarySubtype::Generic,
            bytes: vec![1, 2, 3],
        })),
        CborValue::Bytes(vec![1, 2, 3])
    );
}

#[test]
fn bson_object_id_and_date_collapse_to_text_that_reads_back() {
    let oid = ObjectId::new();
    assert_eq!(
        bson_to_cbor(&Bson::ObjectId(oid)),
        CborValue::Text(oid.to_hex())
    );

    let at = DateTime::parse_rfc3339_str("2026-10-10T08:00:00Z").unwrap();
    let CborValue::Text(text) = bson_to_cbor(&Bson::DateTime(at)) else {
        panic!("a DateTime collapses to text");
    };
    assert_eq!(DateTime::parse_rfc3339_str(&text).unwrap(), at);

    let amount = Decimal128::from_str("3.14").unwrap();
    let CborValue::Text(text) = bson_to_cbor(&Bson::Decimal128(amount)) else {
        panic!("a Decimal128 collapses to text");
    };
    assert_eq!(Decimal128::from_str(&text).unwrap(), amount);
}

#[test]
fn bson_code_and_symbols_collapse_to_text() {
    assert_eq!(
        bson_to_cbor(&Bson::JavaScriptCode("x + 1".into())),
        CborValue::Text("x + 1".into())
    );
    assert_eq!(
        bson_to_cbor(&Bson::Symbol("sym".into())),
        CborValue::Text("sym".into())
    );
}

#[test]
#[ignore = "possible bug: the cbor module doc says a Regex collapses to a string, but it becomes a {$regex, $options} map"]
fn bson_regex_collapses_to_text() {
    let regex = Bson::RegularExpression(bson::Regex {
        pattern: "^a".into(),
        options: "i".into(),
    });
    assert!(matches!(bson_to_cbor(&regex), CborValue::Text(_)));
}

#[test]
fn bson_timestamp_becomes_a_map_with_t_and_i() {
    let ts = Bson::Timestamp(bson::Timestamp {
        time: 5,
        increment: 7,
    });
    assert_eq!(
        bson_to_cbor(&ts),
        CborValue::Map(vec![
            (CborValue::Text("t".into()), int(5)),
            (CborValue::Text("i".into()), int(7)),
        ])
    );
}

#[test]
fn bson_documents_and_arrays_keep_their_order() {
    let doc = Bson::Document(bson::doc! { "name": "Marty", "age": 17_i64 });
    assert_eq!(
        bson_to_cbor(&doc),
        CborValue::Map(vec![
            (
                CborValue::Text("name".into()),
                CborValue::Text("Marty".into())
            ),
            (CborValue::Text("age".into()), int(17)),
        ])
    );
    let arr = Bson::Array(vec![Bson::Int64(1), Bson::Int64(2), Bson::Int64(3)]);
    assert_eq!(
        bson_to_cbor(&arr),
        CborValue::Array(vec![int(1), int(2), int(3)])
    );
}

#[test]
fn cbor_scalars_become_their_bson_counterparts() {
    assert_eq!(cbor_to_bson(&CborValue::Null), Bson::Null);
    assert_eq!(cbor_to_bson(&CborValue::Bool(false)), Bson::Boolean(false));
    assert_eq!(cbor_to_bson(&int(99)), Bson::Int64(99));
    assert_eq!(cbor_to_bson(&CborValue::Float(1.5)), Bson::Double(1.5));
    assert_eq!(
        cbor_to_bson(&CborValue::Text("x".into())),
        Bson::String("x".into())
    );
    assert_eq!(
        cbor_to_bson(&CborValue::Bytes(vec![9, 8])),
        Bson::Binary(Binary {
            subtype: BinarySubtype::Generic,
            bytes: vec![9, 8],
        })
    );
}

#[test]
fn cbor_integers_wider_than_i64_stringify_instead_of_truncating() {
    let wide = CborValue::Integer(u64::MAX.into());
    let Bson::String(text) = cbor_to_bson(&wide) else {
        panic!("a wide integer must not become a number");
    };
    assert!(text.contains("18446744073709551615"), "{text}");
}

#[test]
fn cbor_maps_arrays_and_tags_become_bson_documents_arrays_and_their_content() {
    let map = CborValue::Map(vec![
        (CborValue::Text("k".into()), CborValue::Bool(true)),
        (
            CborValue::Text("list".into()),
            CborValue::Array(vec![int(1), CborValue::Text("two".into())]),
        ),
    ]);
    assert_eq!(
        cbor_to_bson(&map),
        Bson::Document(bson::doc! { "k": true, "list": [1_i64, "two"] })
    );

    let tagged = CborValue::Tag(1, Box::new(int(1_700_000_000)));
    assert_eq!(cbor_to_bson(&tagged), Bson::Int64(1_700_000_000));
}

#[test]
fn a_document_survives_a_trip_through_cbor() {
    let original = Bson::Document(bson::doc! {
        "name": "Doc",
        "year": 1985_i64,
        "active": true,
        "nested": { "tags": ["a", "b"], "ratio": 0.5 },
    });
    assert_eq!(cbor_to_bson(&bson_to_cbor(&original)), original);
}
