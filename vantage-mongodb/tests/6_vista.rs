//! Vista integration: typed `Table<MongoDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running MongoDB. Set `MONGODB_URL` (defaults to
//! `mongodb://localhost:27017`). Each test runs against a fresh randomised
//! database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_mongodb::{AnyMongoType, MongoDB, MongoId};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::VistaFactory;

/// Tests share `Box<dyn Error>` so `?` accepts both `mongodb::error::Error`
/// (raw driver setup) and `vantage_core::Error` (vista calls) uniformly.
type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_{}", ObjectId::new().to_hex());
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

fn product_table(db: MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::<MongoDB, EmptyEntity>::new("product", db)
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

fn rec(fields: &[(&str, AnyMongoType)]) -> Record<AnyMongoType> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[tokio::test]
async fn vista_lists_typed_mongo_as_cbor() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());

    let id = MongoId::from(ObjectId::new());
    table
        .insert_value(
            &id,
            &rec(&[
                ("name", AnyMongoType::new("Cupcake".to_string())),
                ("price", AnyMongoType::new(250i64)),
                ("is_deleted", AnyMongoType::new(false)),
            ]),
        )
        .await?;

    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), "product");
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);

    let key = id.to_string();
    let row = rows.get(&key).expect("row keyed by hex id");
    assert_eq!(
        row.get("name"),
        Some(&CborValue::Text("Cupcake".to_string()))
    );
    assert_eq!(row.get("price"), Some(&CborValue::Integer(250i64.into())));
    assert_eq!(row.get("is_deleted"), Some(&CborValue::Bool(false)));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_get_value_by_id() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());

    let id = MongoId::from(ObjectId::new());
    table
        .insert_value(
            &id,
            &rec(&[("name", AnyMongoType::new("Tart".to_string()))]),
        )
        .await?;

    let vista = db.vista_factory().from_table(table)?;

    let row = vista.get_value(&id.to_string()).await?.expect("found");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Tart".to_string())));

    let missing = vista
        .get_value(&"nonexistenthex000000000000".to_string())
        .await?;
    assert!(missing.is_none());

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_count_with_eq_condition() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());

    for (n, deleted) in [("A", false), ("B", true), ("C", false)] {
        table
            .insert_value(
                &MongoId::from(ObjectId::new()),
                &rec(&[
                    ("name", AnyMongoType::new(n.to_string())),
                    ("is_deleted", AnyMongoType::new(deleted)),
                ]),
            )
            .await?;
    }

    let mut vista = db.vista_factory().from_table(table)?;
    assert_eq!(vista.get_count().await?, 3);

    vista.add_condition_eq("is_deleted", CborValue::Bool(false))?;
    assert_eq!(vista.get_count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_yaml_loads_collection_and_columns() -> TestResult {
    let (db, name) = setup().await;

    db.collection::<bson::Document>("clients")
        .insert_one(bson::doc! {
            "name": "Marty",
            "is_paying_client": true,
        })
        .await?;

    let yaml = r#"
name: client
columns:
  _id:
    type: object_id
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  is_paying_client:
    type: bool
mongo:
  collection: clients
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;

    assert_eq!(vista.name(), "client");
    assert_eq!(vista.get_id_column(), Some("_id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Marty".to_string())));
    assert_eq!(row.get("is_paying_client"), Some(&CborValue::Bool(true)));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_writes_round_trip_via_cbor() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let id = ObjectId::new().to_hex();
    let record: Record<CborValue> = vec![
        ("name".to_string(), CborValue::Text("Pie".into())),
        ("price".to_string(), CborValue::Integer(99i64.into())),
        ("is_deleted".to_string(), CborValue::Bool(false)),
    ]
    .into_iter()
    .collect();

    vista.insert_value(&id, &record).await?;

    let fetched = vista.get_value(&id).await?.expect("inserted");
    assert_eq!(fetched.get("name"), Some(&CborValue::Text("Pie".into())));
    assert_eq!(
        fetched.get("price"),
        Some(&CborValue::Integer(99i64.into()))
    );

    vista.delete(&id).await?;
    assert!(vista.get_value(&id).await?.is_none());

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, name) = setup().await;
    let table = Table::<MongoDB, EmptyEntity>::new("anything", db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(caps.can_insert);
    assert!(caps.can_update);
    assert!(caps.can_delete);
    assert!(!caps.can_subscribe);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_nested_path_reads_writes_and_filters() -> TestResult {
    let (db, name) = setup().await;

    // Seed two raw docs with nested address sub-doc and an aliased fullName.
    db.collection::<bson::Document>("clients")
        .insert_many(vec![
            bson::doc! {
                "fullName": "Marty McFly",
                "address": { "city": "Hill Valley", "zip": "1985" },
            },
            bson::doc! {
                "fullName": "Doc Brown",
                "address": { "city": "Hill Valley", "zip": "1955" },
            },
        ])
        .await?;

    let yaml = r#"
name: client
columns:
  _id:
    type: object_id
    flags: [id]
  full_name:
    type: string
    flags: [title]
    mongo:
      field: fullName
  city:
    type: string
    mongo:
      nested_path: address.city
  zip:
    type: string
    mongo:
      nested_path: address.zip
mongo:
  collection: clients
"#;

    let mut vista = db.vista_factory().from_yaml(yaml)?;

    // Read: nested_path values are surfaced under the spec column name.
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    let marty = rows
        .values()
        .find(|r| r.get("full_name") == Some(&CborValue::Text("Marty McFly".into())))
        .expect("marty present");
    assert_eq!(
        marty.get("city"),
        Some(&CborValue::Text("Hill Valley".into()))
    );
    assert_eq!(marty.get("zip"), Some(&CborValue::Text("1985".into())));
    // Raw BSON keys must not leak through alongside the spec names.
    assert!(marty.get("address").is_none());
    assert!(marty.get("fullName").is_none());

    // Filter: nested_path translates to dot-notation server-side.
    vista.add_condition_eq("zip", CborValue::Text("1985".into()))?;
    assert_eq!(vista.get_count().await?, 1);
    let only = vista.list_values().await?;
    assert_eq!(only.len(), 1);

    // Write: a new vista without the filter, insert via spec names.
    let vista = db.vista_factory().from_yaml(yaml)?;
    let new_id = ObjectId::new().to_hex();
    let record: Record<CborValue> = vec![
        (
            "full_name".to_string(),
            CborValue::Text("Biff Tannen".into()),
        ),
        ("city".to_string(), CborValue::Text("Hill Valley".into())),
        ("zip".to_string(), CborValue::Text("2015".into())),
    ]
    .into_iter()
    .collect();
    vista.insert_value(&new_id, &record).await?;

    // Verify the raw BSON has the nested structure (not flattened keys).
    let raw = db
        .collection::<bson::Document>("clients")
        .find_one(bson::doc! { "_id": ObjectId::parse_str(&new_id)? })
        .await?
        .expect("inserted doc present");
    assert_eq!(raw.get_str("fullName")?, "Biff Tannen");
    let address = raw.get_document("address")?;
    assert_eq!(address.get_str("city")?, "Hill Valley");
    assert_eq!(address.get_str("zip")?, "2015");
    assert!(raw.get("city").is_none());
    assert!(raw.get("zip").is_none());

    // And reading it back through the vista surfaces the spec names again.
    let read = vista.get_value(&new_id).await?.expect("read");
    assert_eq!(
        read.get("full_name"),
        Some(&CborValue::Text("Biff Tannen".into()))
    );
    assert_eq!(read.get("zip"), Some(&CborValue::Text("2015".into())));

    teardown(&db, &name).await;
    Ok(())
}
