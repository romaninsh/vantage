//! Stored BSON types surviving Vista read-modify-write: values the CBOR
//! surface reads as text (`ObjectId`, `DateTime`, `Decimal128`) are written
//! back in their stored type on columns that declare no type. Requires a
//! running MongoDB (`MONGODB_URL`, defaults to `mongodb://localhost:27017`);
//! each test runs against a fresh randomised database and drops it on the way
//! out.

#![cfg(feature = "vista")]

use std::error::Error;
use std::str::FromStr;
use std::sync::Arc;

use bson::oid::ObjectId;
use bson::{Bson, DateTime, Decimal128, Document, doc};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{AnyMongoType, MongoDB};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{SpecResolver, Vista, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_preserve_{}", ObjectId::new().to_hex());
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

fn spec(yaml: &str) -> MongoVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("spec yaml")
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

/// One event whose fields carry every BSON type the CBOR surface reads as
/// text (at the top level, inside an array, and in a nested document), an
/// Int32, and a DateTime outside the RFC 3339 range, which reads as `""`.
async fn seed(db: &MongoDB) -> std::result::Result<Document, Box<dyn Error>> {
    let seed = doc! {
        "_id": "e1",
        "client_id": ObjectId::new(),
        "when": DateTime::parse_rfc3339_str("2026-10-10T12:30:00Z")?,
        "far": DateTime::from_millis(i64::MAX),
        "amount": Decimal128::from_str("12.50")?,
        "count": 3i32,
        "tags": [ObjectId::new(), ObjectId::new()],
        "place": { "venue_id": ObjectId::new(), "label": "hall" },
        "note": "first",
    };
    db.doc_collection("events").insert_one(seed.clone()).await?;
    Ok(seed)
}

fn assert_types_kept(stored: &Document, seed: &Document) {
    for (key, value) in seed.iter().filter(|(k, _)| *k != "note") {
        assert_eq!(stored.get(key), Some(value), "{key}: {stored:?}");
    }
}

/// Read e1, write it back unchanged, then patch the full row with one field
/// changed and with that field alone; the stored types survive each write.
async fn round_trip(db: &MongoDB, vista: &Vista, seed: &Document) -> TestResult {
    let row = vista.get_value("e1").await?.expect("e1");
    let client = seed.get_object_id("client_id")?;
    assert_eq!(row.get("client_id"), Some(&text(&client.to_hex())));

    vista.replace_value("e1", &row).await?;
    assert_types_kept(&raw(db, "e1").await, seed);

    let mut edited = row.clone();
    edited.insert("note".into(), text("second"));
    vista.patch_value("e1", &edited).await?;
    let stored = raw(db, "e1").await;
    assert_types_kept(&stored, seed);
    assert_eq!(stored.get_str("note")?, "second");

    let mut partial = Record::new();
    partial.insert("note".to_string(), text("third"));
    vista.patch_value("e1", &partial).await?;
    assert_types_kept(&raw(db, "e1").await, seed);
    Ok(())
}

#[tokio::test]
async fn read_modify_write_preserves_object_id_date_decimal() -> TestResult {
    let (db, name) = setup().await;
    let seed = seed(&db).await?;

    let typed = Table::<MongoDB, EmptyEntity>::new("events", db.clone())
        .with_column_of::<String>("client_id")
        .with_column_of::<AnyMongoType>("when")
        .with_column_of::<AnyMongoType>("far")
        .with_column_of::<AnyMongoType>("amount")
        .with_column_of::<i64>("count")
        .with_column_of::<AnyMongoType>("tags")
        .with_column_of::<AnyMongoType>("place")
        .with_column_of::<String>("note");
    round_trip(&db, &db.vista_factory().from_table(typed)?, &seed).await?;

    let bare = Table::<MongoDB, EmptyEntity>::new("events", db.clone());
    round_trip(&db, &db.vista_factory().from_table(bare)?, &seed).await?;

    // The nested venue rides on its own spec column here.
    let yaml = db.vista_factory().build_from_spec(spec(
        r#"
name: event
columns:
  _id: { flags: [id] }
  client_id: {}
  when: {}
  far: {}
  amount: {}
  count: {}
  tags: {}
  venue_id: { mongo: { nested_path: place.venue_id } }
  label: { mongo: { nested_path: place.label } }
  note: {}
mongo:
  collection: events
"#,
    ))?;
    round_trip(&db, &yaml, &seed).await?;

    // A changed value that still parses as the stored type keeps it.
    let other = ObjectId::new();
    let mut partial = Record::new();
    partial.insert("client_id".to_string(), text(&other.to_hex()));
    partial.insert("amount".to_string(), text("99.95"));
    partial.insert("count".to_string(), CborValue::Integer(4i64.into()));
    yaml.patch_value("e1", &partial).await?;
    let stored = raw(&db, "e1").await;
    assert_eq!(stored.get("client_id"), Some(&Bson::ObjectId(other)));
    assert_eq!(
        stored.get("amount"),
        Some(&Bson::Decimal128(Decimal128::from_str("99.95")?))
    );
    assert_eq!(stored.get("count"), Some(&Bson::Int32(4)));

    // Text that does not parse as the stored type is written as text.
    let mut partial = Record::new();
    partial.insert("when".to_string(), text("soon"));
    yaml.patch_value("e1", &partial).await?;
    assert_eq!(raw(&db, "e1").await.get_str("when")?, "soon");

    teardown(&db, &name).await;
    Ok(())
}

const CLIENT: &str = r#"
name: client
columns:
  _id: { flags: [id] }
  name: { flags: [title] }
mongo:
  collection: clients
"#;

const ORDER: &str = r#"
name: order
columns:
  _id: { flags: [id] }
  client_id: {}
  item: { flags: [title] }
  client.name: {}
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#;

fn resolver() -> SpecResolver<MongoVistaSpec> {
    let map: IndexMap<String, MongoVistaSpec> = [CLIENT, ORDER]
        .into_iter()
        .map(|y| {
            let s = spec(y);
            (s.name.clone(), s)
        })
        .collect();
    Arc::new(move |name: &str| map.get(name).cloned())
}

#[tokio::test]
async fn join_survives_vista_edit() -> TestResult {
    let (db, name) = setup().await;
    let marty = ObjectId::new();
    db.doc_collection("clients")
        .insert_one(doc! { "_id": marty, "name": "Marty" })
        .await?;
    db.doc_collection("orders")
        .insert_one(doc! { "_id": "o1", "client_id": marty, "item": "skateboard" })
        .await?;
    let orders = db
        .vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(ORDER))?;

    let mut row = orders.get_value("o1").await?.expect("o1");
    row.insert("item".into(), text("hoverboard"));
    orders.replace_value("o1", &row).await?;
    let mut row = orders.get_value("o1").await?.expect("o1");
    assert_eq!(row.get("client.name"), Some(&text("Marty")));

    row.insert("item".into(), text("almanac"));
    row.shift_remove("client.name");
    orders.patch_value("o1", &row).await?;
    let row = orders.get_value("o1").await?.expect("o1");
    assert_eq!(row.get("client.name"), Some(&text("Marty")));
    assert_eq!(row.get("item"), Some(&text("almanac")));

    let stored = db
        .doc_collection("orders")
        .find_one(doc! { "_id": "o1" })
        .await?
        .expect("stored o1");
    assert_eq!(stored.get("client_id"), Some(&Bson::ObjectId(marty)));

    teardown(&db, &name).await;
    Ok(())
}
