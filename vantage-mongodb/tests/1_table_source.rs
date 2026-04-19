//! Integration tests for MongoDB TableSource.
//!
//! Requires a running MongoDB instance. Set MONGODB_URL env var or defaults
//! to mongodb://localhost:27017. Uses a randomised database name so tests
//! don't collide.

use bson::doc;
use vantage_dataset::prelude::*;
use vantage_mongodb::MongoId;
use vantage_mongodb::{AnyMongoType, MongoDB};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_test_{}", bson::oid::ObjectId::new().to_hex());
    let db = MongoDB::connect(&mongo_url(), &db_name)
        .await
        .expect("Failed to connect to MongoDB");
    (db, db_name)
}

async fn teardown(db: &MongoDB, db_name: &str) {
    db.database()
        .drop()
        .await
        .unwrap_or_else(|e| eprintln!("Failed to drop test db {}: {}", db_name, e));
}

fn product_table(db: MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::new("product", db)
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<i64>("calories")
        .with_column_of::<bool>("is_deleted")
}

fn record(fields: &[(&str, AnyMongoType)]) -> Record<AnyMongoType> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

// ── Read operations ──────────────────────────────────────────────────

#[tokio::test]
async fn test_list_empty() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let values = table.list_values().await.unwrap();
    assert_eq!(values.len(), 0);

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_insert_and_list() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let id = MongoId::from(bson::oid::ObjectId::new());
    let rec = record(&[
        ("name", AnyMongoType::new("Cupcake".to_string())),
        ("price", AnyMongoType::new(250i64)),
        ("calories", AnyMongoType::new(300i64)),
        ("is_deleted", AnyMongoType::new(false)),
    ]);

    table.insert_value(&id, &rec).await.unwrap();

    let values = table.list_values().await.unwrap();
    assert_eq!(values.len(), 1);
    assert!(values.contains_key(&id));

    let fetched = &values[&id];
    assert_eq!(fetched["name"].try_get::<String>(), Some("Cupcake".into()));
    assert_eq!(fetched["price"].try_get::<i64>(), Some(250));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_get_value_by_id() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let id = MongoId::from(bson::oid::ObjectId::new());
    let rec = record(&[
        ("name", AnyMongoType::new("Tart".to_string())),
        ("price", AnyMongoType::new(180i64)),
        ("calories", AnyMongoType::new(200i64)),
        ("is_deleted", AnyMongoType::new(false)),
    ]);
    table.insert_value(&id, &rec).await.unwrap();

    let fetched = table.get_value(&id).await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Tart".into()));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_get_some_value() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    // Empty table → None
    assert!(table.get_some_value().await.unwrap().is_none());

    let id = MongoId::from(bson::oid::ObjectId::new());
    let rec = record(&[("name", AnyMongoType::new("Pie".to_string()))]);
    table.insert_value(&id, &rec).await.unwrap();

    let (got_id, got_rec) = table.get_some_value().await.unwrap().unwrap();
    assert_eq!(got_id, id);
    assert_eq!(got_rec["name"].try_get::<String>(), Some("Pie".into()));

    teardown(&db, &db_name).await;
}

// ── Count ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_count() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    assert_eq!(table.get_count().await.unwrap(), 0);

    for i in 0..3 {
        let rec = record(&[("name", AnyMongoType::new(format!("Item {}", i)))]);
        table
            .insert_value(&MongoId::from(bson::oid::ObjectId::new()), &rec)
            .await
            .unwrap();
    }

    assert_eq!(table.get_count().await.unwrap(), 3);

    teardown(&db, &db_name).await;
}

// ── Aggregates ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_sum_max_min() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let prices = [100i64, 250, 400];
    for p in prices {
        let rec = record(&[
            ("name", AnyMongoType::new(format!("P{}", p))),
            ("price", AnyMongoType::new(p)),
        ]);
        table
            .insert_value(&MongoId::from(bson::oid::ObjectId::new()), &rec)
            .await
            .unwrap();
    }

    let price_col = db.to_any_column(db.create_column::<i64>("price"));

    let sum = db.get_table_sum(&table, &price_col).await.unwrap();
    assert_eq!(sum.try_get::<i64>(), Some(750));

    let max = db.get_table_max(&table, &price_col).await.unwrap();
    assert_eq!(max.try_get::<i64>(), Some(400));

    let min = db.get_table_min(&table, &price_col).await.unwrap();
    assert_eq!(min.try_get::<i64>(), Some(100));

    teardown(&db, &db_name).await;
}

// ── Write operations ─────────────────────────────────────────────────

#[tokio::test]
async fn test_replace() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let id = MongoId::from(bson::oid::ObjectId::new());
    let rec = record(&[
        ("name", AnyMongoType::new("Old".to_string())),
        ("price", AnyMongoType::new(10i64)),
    ]);
    table.insert_value(&id, &rec).await.unwrap();

    let replacement = record(&[
        ("name", AnyMongoType::new("New".to_string())),
        ("price", AnyMongoType::new(99i64)),
    ]);
    table.replace_value(&id, &replacement).await.unwrap();

    let fetched = table.get_value(&id).await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("New".into()));
    assert_eq!(fetched["price"].try_get::<i64>(), Some(99));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_patch() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let id = MongoId::from(bson::oid::ObjectId::new());
    let rec = record(&[
        ("name", AnyMongoType::new("Original".to_string())),
        ("price", AnyMongoType::new(50i64)),
        ("calories", AnyMongoType::new(200i64)),
    ]);
    table.insert_value(&id, &rec).await.unwrap();

    // Patch only the price — name and calories should be untouched
    let patch = record(&[("price", AnyMongoType::new(75i64))]);
    table.patch_value(&id, &patch).await.unwrap();

    let fetched = table.get_value(&id).await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Original".into()));
    assert_eq!(fetched["price"].try_get::<i64>(), Some(75));
    assert_eq!(fetched["calories"].try_get::<i64>(), Some(200));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_delete() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let id = MongoId::from(bson::oid::ObjectId::new());
    let rec = record(&[("name", AnyMongoType::new("Gone".to_string()))]);
    table.insert_value(&id, &rec).await.unwrap();
    assert_eq!(table.get_count().await.unwrap(), 1);

    WritableValueSet::delete(&table, &id).await.unwrap();
    assert_eq!(table.get_count().await.unwrap(), 0);

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_delete_all() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    for i in 0..4 {
        let rec = record(&[("name", AnyMongoType::new(format!("X{}", i)))]);
        table
            .insert_value(&MongoId::from(bson::oid::ObjectId::new()), &rec)
            .await
            .unwrap();
    }
    assert_eq!(table.get_count().await.unwrap(), 4);

    WritableValueSet::delete_all(&table).await.unwrap();
    assert_eq!(table.get_count().await.unwrap(), 0);

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_insert_return_id() {
    let (db, db_name) = setup().await;
    let table = product_table(db.clone());

    let rec = record(&[
        ("name", AnyMongoType::new("Auto".to_string())),
        ("price", AnyMongoType::new(42i64)),
    ]);
    let id = table.insert_return_id_value(&rec).await.unwrap();

    let fetched = table.get_value(&id).await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Auto".into()));

    teardown(&db, &db_name).await;
}

// ── Conditions (native bson::Document) ───────────────────────────────

#[tokio::test]
async fn test_condition_filter() {
    let (db, db_name) = setup().await;
    let mut table = product_table(db.clone());

    // Insert mix of deleted / not deleted
    for (name, deleted) in [("A", false), ("B", true), ("C", false)] {
        let rec = record(&[
            ("name", AnyMongoType::new(name.to_string())),
            ("is_deleted", AnyMongoType::new(deleted)),
        ]);
        table
            .insert_value(&MongoId::from(bson::oid::ObjectId::new()), &rec)
            .await
            .unwrap();
    }

    // Add native MongoDB condition
    table.add_condition(doc! { "is_deleted": false });

    assert_eq!(table.get_count().await.unwrap(), 2);

    let values = table.list_values().await.unwrap();
    assert_eq!(values.len(), 2);
    let names: Vec<String> = values
        .values()
        .filter_map(|r| r["name"].try_get::<String>())
        .collect();
    assert!(names.contains(&"A".to_string()));
    assert!(names.contains(&"C".to_string()));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_condition_gt() {
    let (db, db_name) = setup().await;
    let mut table = product_table(db.clone());

    for (name, price) in [("Cheap", 50i64), ("Mid", 150), ("Expensive", 300)] {
        let rec = record(&[
            ("name", AnyMongoType::new(name.to_string())),
            ("price", AnyMongoType::new(price)),
        ]);
        table
            .insert_value(&MongoId::from(bson::oid::ObjectId::new()), &rec)
            .await
            .unwrap();
    }

    table.add_condition(doc! { "price": { "$gt": 100 } });

    assert_eq!(table.get_count().await.unwrap(), 2);

    let values = table.list_values().await.unwrap();
    let names: Vec<String> = values
        .values()
        .filter_map(|r| r["name"].try_get::<String>())
        .collect();
    assert!(names.contains(&"Mid".to_string()));
    assert!(names.contains(&"Expensive".to_string()));
    assert!(!names.contains(&"Cheap".to_string()));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn test_multiple_conditions() {
    let (db, db_name) = setup().await;
    let mut table = product_table(db.clone());

    for (name, price, deleted) in [
        ("A", 50i64, false),
        ("B", 200, false),
        ("C", 300, true),
        ("D", 400, false),
    ] {
        let rec = record(&[
            ("name", AnyMongoType::new(name.to_string())),
            ("price", AnyMongoType::new(price)),
            ("is_deleted", AnyMongoType::new(deleted)),
        ]);
        table
            .insert_value(&MongoId::from(bson::oid::ObjectId::new()), &rec)
            .await
            .unwrap();
    }

    // price > 100 AND not deleted
    table.add_condition(doc! { "price": { "$gt": 100 } });
    table.add_condition(doc! { "is_deleted": false });

    assert_eq!(table.get_count().await.unwrap(), 2);

    let values = table.list_values().await.unwrap();
    let names: Vec<String> = values
        .values()
        .filter_map(|r| r["name"].try_get::<String>())
        .collect();
    assert!(names.contains(&"B".to_string()));
    assert!(names.contains(&"D".to_string()));

    teardown(&db, &db_name).await;
}
