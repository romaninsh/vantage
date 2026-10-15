//! YAML columns declared `object_id`, `datetime` (`timestamp`) or `decimal`
//! parse written text into their BSON type, refuse text that does not parse,
//! and leave stored data that does not fit the type alone when it is written
//! back unchanged. Requires a running MongoDB (`MONGODB_URL`, defaults to
//! `mongodb://localhost:27017`); each test runs against a fresh randomised
//! database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;
use std::str::FromStr;

use bson::oid::ObjectId;
use bson::{Bson, DateTime, Decimal128, Document, doc};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_mongodb::MongoDB;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_types::Record;
use vantage_vista::{Vista, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_declared_{}", ObjectId::new().to_hex());
    let db = MongoDB::connect(&mongo_url(), &db_name)
        .await
        .expect("connect mongo");
    (db, db_name)
}

async fn teardown(db: &MongoDB, db_name: &str) {
    db.database()
        .drop()
        .await
        .unwrap_or_else(|e| eprintln!("drop {db_name}: {e}"));
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

async fn raw(db: &MongoDB, id: &str) -> Document {
    db.doc_collection("events")
        .find_one(doc! { "_id": id })
        .await
        .expect("find_one")
        .expect("stored document")
}

const TYPED_EVENTS: &str = r#"
name: event
columns:
  _id: { flags: [id] }
  client_id: { type: object_id }
  when: { type: datetime }
  logged: { type: timestamp }
  amount: { type: decimal }
  tags: { type: object_id }
  venue_id: { type: object_id, mongo: { nested_path: place.venue_id } }
  note: {}
mongo:
  collection: events
"#;

fn typed_events(db: &MongoDB) -> Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(TYPED_EVENTS).expect("spec yaml");
    db.vista_factory()
        .build_from_spec(spec)
        .expect("typed events vista")
}

fn typed_row(client: ObjectId, tag: ObjectId, venue: ObjectId) -> Record<CborValue> {
    let mut row = Record::new();
    row.insert("client_id".into(), text(&client.to_hex()));
    row.insert("when".into(), text("2026-10-10T08:00:00Z"));
    row.insert(
        "logged".into(),
        CborValue::Tag(1, Box::new(CborValue::Integer(1_760_083_200i64.into()))),
    );
    row.insert("amount".into(), text("3.14"));
    row.insert("tags".into(), CborValue::Array(vec![text(&tag.to_hex())]));
    row.insert("venue_id".into(), text(&venue.to_hex()));
    row.insert("note".into(), text("24 chars hex-free note!!"));
    row
}

fn expected(client: ObjectId, tag: ObjectId, venue: ObjectId) -> Document {
    doc! {
        "client_id": client,
        "when": DateTime::parse_rfc3339_str("2026-10-10T08:00:00Z").unwrap(),
        "logged": DateTime::from_millis(1_760_083_200_000),
        "amount": Decimal128::from_str("3.14").unwrap(),
        "tags": [tag],
        "place": { "venue_id": venue },
        "note": "24 chars hex-free note!!",
    }
}

fn assert_fields(stored: &Document, expected: &Document) {
    for (key, value) in expected {
        assert_eq!(stored.get(key), Some(value), "{key}: {stored:?}");
    }
}

#[tokio::test]
async fn declared_types_coerce_on_insert() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    let (client, tag, venue) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    let row = typed_row(client, tag, venue);
    let want = expected(client, tag, venue);

    vista.insert_value("e2", &row).await?;
    assert_fields(&raw(&db, "e2").await, &want);

    vista.replace_value("e3", &row).await?;
    assert_fields(&raw(&db, "e3").await, &want);

    let mut records = IndexMap::new();
    records.insert("e4".to_string(), row.clone());
    assert_eq!(vista.import_values(&records).await?, 1);
    assert_fields(&raw(&db, "e4").await, &want);

    // A number is a decimal too; null stays null.
    let mut partial = Record::new();
    partial.insert("amount".to_string(), CborValue::Integer(7i64.into()));
    partial.insert("when".to_string(), CborValue::Null);
    vista.patch_value("e2", &partial).await?;
    let stored = raw(&db, "e2").await;
    assert_eq!(
        stored.get("amount"),
        Some(&Bson::Decimal128(Decimal128::from_str("7")?))
    );
    assert_eq!(stored.get("when"), Some(&Bson::Null));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_type_rejects_bad_text() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    let mut row = typed_row(ObjectId::new(), ObjectId::new(), ObjectId::new());
    row.insert("when".into(), text("not a date"));

    let err = vista.insert_value("e5", &row).await.expect_err("bad date");
    assert!(err.is_conflict(), "{err}");
    assert!(err.to_string().contains("column type"), "{err}");
    assert!(err.to_string().contains("when"), "{err}");

    let mut bad_id = typed_row(ObjectId::new(), ObjectId::new(), ObjectId::new());
    bad_id.insert("tags".into(), CborValue::Array(vec![text("nope")]));
    assert!(vista.insert_value("e6", &bad_id).await.is_err());

    let mut records = IndexMap::new();
    records.insert("e7".to_string(), row.clone());
    assert!(vista.import_values(&records).await.is_err());
    assert_eq!(
        db.doc_collection("events").count_documents(doc! {}).await?,
        0
    );

    // Replace and patch of a stored row refuse it too, writing nothing.
    let good = typed_row(ObjectId::new(), ObjectId::new(), ObjectId::new());
    vista.insert_value("e8", &good).await?;
    let before = raw(&db, "e8").await;
    let err = vista.replace_value("e8", &row).await.expect_err("replace");
    assert!(err.is_conflict(), "{err}");
    let mut partial = Record::new();
    partial.insert("note".to_string(), text("changed"));
    partial.insert("when".to_string(), text("not a date"));
    let err = vista.patch_value("e8", &partial).await.expect_err("patch");
    assert!(err.is_conflict(), "{err}");
    assert_eq!(raw(&db, "e8").await, before);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_type_keeps_nonconforming_stored_data() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    // A legacy string id, and a DateTime outside the RFC 3339 range (which
    // reads as `""`), in columns declared `object_id` and `datetime`.
    let legacy = doc! {
        "_id": "e9",
        "client_id": "c1",
        "when": DateTime::from_millis(i64::MAX),
        "note": "legacy",
    };
    db.doc_collection("events")
        .insert_one(legacy.clone())
        .await?;

    let row = vista.get_value("e9").await?.expect("e9");
    assert_eq!(row.get("client_id"), Some(&text("c1")));
    assert_eq!(row.get("when"), Some(&text("")));
    vista.replace_value("e9", &row).await?;
    assert_fields(&raw(&db, "e9").await, &legacy);

    let mut edited = row.clone();
    edited.insert("note".into(), text("edited"));
    vista.patch_value("e9", &edited).await?;
    let stored = raw(&db, "e9").await;
    assert_eq!(stored.get("client_id"), Some(&Bson::String("c1".into())));
    assert_eq!(stored.get("when"), legacy.get("when"));
    assert_eq!(stored.get_str("note")?, "edited");

    teardown(&db, &name).await;
    Ok(())
}

fn tag(n: u64, inner: CborValue) -> CborValue {
    CborValue::Tag(n, Box::new(inner))
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn row_with(column: &str, value: CborValue) -> Record<CborValue> {
    let mut row = typed_row(ObjectId::new(), ObjectId::new(), ObjectId::new());
    row.insert(column.to_string(), value);
    row
}

#[tokio::test]
async fn declared_datetime_accepts_text_tagged_text_and_epoch_seconds() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    let at = DateTime::parse_rfc3339_str("2026-10-10T08:00:00Z")?;

    vista
        .insert_value(
            "d1",
            &row_with("when", tag(0, text("2026-10-10T08:00:00Z"))),
        )
        .await?;
    assert_eq!(raw(&db, "d1").await.get("when"), Some(&Bson::DateTime(at)));

    vista
        .insert_value("d2", &row_with("when", tag(1, int(1_760_083_200))))
        .await?;
    assert_eq!(
        raw(&db, "d2").await.get("when"),
        Some(&Bson::DateTime(DateTime::from_millis(1_760_083_200_000)))
    );

    // Fractions of a second survive down to the millisecond.
    vista
        .insert_value(
            "d3",
            &row_with("when", tag(1, CborValue::Float(1_760_083_200.25))),
        )
        .await?;
    assert_eq!(
        raw(&db, "d3").await.get("when"),
        Some(&Bson::DateTime(DateTime::from_millis(1_760_083_200_250)))
    );

    // Finer than a millisecond is dropped.
    vista
        .insert_value("d4", &row_with("when", text("2026-10-10T08:00:00.123456Z")))
        .await?;
    assert_eq!(
        raw(&db, "d4").await.get("when"),
        Some(&Bson::DateTime(DateTime::from_millis(
            at.timestamp_millis() + 123
        )))
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_datetime_refuses_values_it_cannot_read_as_a_date() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);

    let refused = [
        tag(1, text("soon")),
        tag(0, int(5)),
        tag(1, CborValue::Float(f64::NAN)),
        tag(1, CborValue::Float(1e300)),
        tag(1, int(i64::MAX)),
        CborValue::Bool(true),
        int(1_760_083_200),
    ];
    for (i, value) in refused.into_iter().enumerate() {
        let err = vista
            .insert_value(format!("r{i}"), &row_with("when", value.clone()))
            .await
            .expect_err("a value that is no date must be refused");
        assert!(err.is_conflict(), "{value:?}: {err}");
        assert!(err.to_string().contains("when"), "{value:?}: {err}");
        assert!(err.to_string().contains("datetime"), "{value:?}: {err}");
    }
    assert_eq!(
        db.doc_collection("events").count_documents(doc! {}).await?,
        0
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_decimal_accepts_text_numbers_bignums_and_decimal_fractions() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    let dec = |s: &str| Bson::Decimal128(Decimal128::from_str(s).unwrap());

    let cases = [
        ("m1", CborValue::Float(2.5), "2.5"),
        ("m2", int(-12), "-12"),
        ("m3", tag(2, CborValue::Bytes(vec![1, 0])), "256"),
        ("m4", tag(3, CborValue::Bytes(vec![1, 0])), "-257"),
        (
            "m5",
            tag(4, CborValue::Array(vec![int(-2), int(31415)])),
            "314.15",
        ),
        (
            "m6",
            tag(
                4,
                CborValue::Array(vec![int(-1), tag(2, CborValue::Bytes(vec![1, 0]))]),
            ),
            "25.6",
        ),
    ];
    for (id, value, want) in cases {
        vista.insert_value(id, &row_with("amount", value)).await?;
        assert_eq!(raw(&db, id).await.get("amount"), Some(&dec(want)), "{id}");
    }

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_decimal_refuses_values_that_are_no_number() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);

    let refused = [
        text("twelve"),
        CborValue::Bool(true),
        CborValue::Float(f64::INFINITY),
        tag(4, CborValue::Array(vec![int(-2)])),
        tag(4, text("31415e-2")),
        tag(2, text("not bytes")),
    ];
    for (i, value) in refused.into_iter().enumerate() {
        let err = vista
            .insert_value(format!("r{i}"), &row_with("amount", value.clone()))
            .await
            .expect_err("a value that is no number must be refused");
        assert!(err.is_conflict(), "{value:?}: {err}");
        assert!(err.to_string().contains("decimal"), "{value:?}: {err}");
    }

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_object_id_refusal_names_the_column_and_type() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);

    let err = vista
        .insert_value("o1", &row_with("client_id", text("not-hex")))
        .await
        .expect_err("text that is no ObjectId must be refused");
    assert!(err.is_conflict(), "{err}");
    assert!(err.to_string().contains("client_id"), "{err}");
    assert!(err.to_string().contains("object_id"), "{err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_array_keeps_unchanged_elements_as_stored_and_types_the_rest() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    let (first, second) = (ObjectId::new(), ObjectId::new());
    // A legacy element in an `object_id` array column: a string.
    db.doc_collection("events")
        .insert_one(doc! { "_id": "a1", "tags": [first, "legacy"] })
        .await?;

    // Written back as read, nothing changes.
    let row = vista.get_value("a1").await?.expect("a1");
    assert_eq!(
        row.get("tags"),
        Some(&CborValue::Array(vec![
            text(&first.to_hex()),
            text("legacy")
        ]))
    );
    vista.replace_value("a1", &row).await?;
    assert_eq!(
        raw(&db, "a1").await.get("tags"),
        Some(&Bson::Array(vec![
            Bson::ObjectId(first),
            Bson::String("legacy".into())
        ]))
    );

    // Change only the first element: the legacy one still keeps its stored value.
    let mut edited = row.clone();
    edited.insert(
        "tags".into(),
        CborValue::Array(vec![text(&second.to_hex()), text("legacy")]),
    );
    vista.replace_value("a1", &edited).await?;
    assert_eq!(
        raw(&db, "a1").await.get("tags"),
        Some(&Bson::Array(vec![
            Bson::ObjectId(second),
            Bson::String("legacy".into())
        ]))
    );

    // A different length is typed as a whole, so the legacy element is refused.
    let mut longer = row.clone();
    longer.insert(
        "tags".into(),
        CborValue::Array(vec![
            text(&second.to_hex()),
            text("legacy"),
            text(&first.to_hex()),
        ]),
    );
    let err = vista
        .replace_value("a1", &longer)
        .await
        .expect_err("a legacy element is refused once the array changes length");
    assert!(err.is_conflict(), "{err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_empty_text_and_null_clear_inside_an_array_too() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    let tag_id = ObjectId::new();
    let row = row_with(
        "tags",
        CborValue::Array(vec![text(&tag_id.to_hex()), text(""), CborValue::Null]),
    );
    vista.insert_value("n1", &row).await?;
    assert_eq!(
        raw(&db, "n1").await.get("tags"),
        Some(&Bson::Array(vec![
            Bson::ObjectId(tag_id),
            Bson::Null,
            Bson::Null
        ]))
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_decimal_refusal_in_a_patch_writes_nothing() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    vista
        .insert_value("p1", &row_with("amount", text("1.50")))
        .await?;
    let before = raw(&db, "p1").await;

    let mut partial = Record::new();
    partial.insert("amount".to_string(), text("lots"));
    let err = vista
        .patch_value("p1", &partial)
        .await
        .expect_err("text that is no decimal must be refused");
    assert!(err.is_conflict(), "{err}");
    assert_eq!(raw(&db, "p1").await, before);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_decimal_empty_text_clears() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    vista
        .insert_value("c1", &row_with("amount", text("")))
        .await?;
    assert_eq!(raw(&db, "c1").await.get("amount"), Some(&Bson::Null));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn declared_type_empty_text_clears() -> TestResult {
    let (db, name) = setup().await;
    let vista = typed_events(&db);
    let row = typed_row(ObjectId::new(), ObjectId::new(), ObjectId::new());
    vista.insert_value("e10", &row).await?;

    let mut edited = vista.get_value("e10").await?.expect("e10");
    edited.insert("when".into(), text(""));
    edited.insert("amount".into(), text(""));
    vista.patch_value("e10", &edited).await?;
    let stored = raw(&db, "e10").await;
    assert_eq!(stored.get("when"), Some(&Bson::Null));
    assert_eq!(stored.get("amount"), Some(&Bson::Null));
    assert!(matches!(stored.get("client_id"), Some(Bson::ObjectId(_))));

    teardown(&db, &name).await;
    Ok(())
}
