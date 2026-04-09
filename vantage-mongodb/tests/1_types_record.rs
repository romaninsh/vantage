//! Test 1b: Record<AnyMongoType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.

use bson::Bson;
use vantage_mongodb::AnyMongoType;
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
