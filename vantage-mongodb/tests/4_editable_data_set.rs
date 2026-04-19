//! Test 4: WritableValueSet and InsertableValueSet for Table<MongoDB, EmptyEntity>.
//!
//! Uses temporary collections (cleaned up per test) to avoid mutating seeded data.

use vantage_dataset::prelude::*;
use vantage_mongodb::{AnyMongoType, MongoDB, MongoId};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup(suffix: &str) -> (MongoDB, Table<MongoDB, EmptyEntity>) {
    let db = MongoDB::connect(&mongo_url(), "vantage").await.unwrap();
    let table_name = format!("edit_item_{}", suffix);

    // Clean slate
    let coll = db.doc_collection(&table_name);
    coll.drop().await.ok();

    // Seed two records
    coll.insert_many(vec![
        bson::doc! { "_id": "a", "name": "Alpha", "price": 10_i64 },
        bson::doc! { "_id": "b", "name": "Beta", "price": 20_i64 },
    ])
    .await
    .unwrap();

    let table = Table::<MongoDB, EmptyEntity>::new(&table_name, db.clone())
        .with_id_column("_id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");

    (db, table)
}

fn record(fields: &[(&str, AnyMongoType)]) -> vantage_types::Record<AnyMongoType> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[tokio::test]
async fn test_insert() {
    let (_db, table) = setup("insert").await;

    let rec = record(&[
        ("name", AnyMongoType::new("Gamma".to_string())),
        ("price", AnyMongoType::new(30i64)),
    ]);
    let result = table.insert_value(&MongoId::from("c"), &rec).await.unwrap();
    assert_eq!(result["name"].try_get::<String>(), Some("Gamma".into()));
    assert_eq!(result["price"].try_get::<i64>(), Some(30));

    let fetched = table
        .get_value(&MongoId::from("c"))
        .await
        .unwrap()
        .expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Gamma".into()));
}

#[tokio::test]
async fn test_replace() {
    let (_db, table) = setup("replace").await;

    let rec = record(&[
        ("name", AnyMongoType::new("Alpha Replaced".to_string())),
        ("price", AnyMongoType::new(99i64)),
    ]);
    table
        .replace_value(&MongoId::from("a"), &rec)
        .await
        .unwrap();

    let fetched = table
        .get_value(&MongoId::from("a"))
        .await
        .unwrap()
        .expect("row a exists");
    assert_eq!(
        fetched["name"].try_get::<String>(),
        Some("Alpha Replaced".into())
    );
    assert_eq!(fetched["price"].try_get::<i64>(), Some(99));
}

#[tokio::test]
async fn test_patch() {
    let (_db, table) = setup("patch").await;

    let partial = record(&[("price", AnyMongoType::new(55i64))]);
    table
        .patch_value(&MongoId::from("a"), &partial)
        .await
        .unwrap();

    let fetched = table
        .get_value(&MongoId::from("a"))
        .await
        .unwrap()
        .expect("row a exists");
    assert_eq!(fetched["price"].try_get::<i64>(), Some(55));
    // name untouched
    assert_eq!(fetched["name"].try_get::<String>(), Some("Alpha".into()));
}

#[tokio::test]
async fn test_delete() {
    let (_db, table) = setup("delete").await;

    WritableValueSet::delete(&table, &MongoId::from("a"))
        .await
        .unwrap();

    let all = table.list_values().await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(!all.contains_key(&MongoId::from("a")));
}

#[tokio::test]
async fn test_delete_all() {
    let (_db, table) = setup("delete_all").await;

    WritableValueSet::delete_all(&table).await.unwrap();
    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_insert_return_id() {
    let (db, _) = setup("auto_id").await;

    let coll_name = "edit_auto_item";
    db.doc_collection(coll_name).drop().await.ok();

    let table = Table::<MongoDB, EmptyEntity>::new(coll_name, db)
        .with_id_column("_id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");

    let rec = record(&[
        ("name", AnyMongoType::new("Auto".to_string())),
        ("price", AnyMongoType::new(42i64)),
    ]);
    let id = table.insert_return_id_value(&rec).await.unwrap();
    assert!(!id.to_string().is_empty());

    let fetched = table.get_value(&id).await.unwrap().expect("row exists");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Auto".into()));
    assert_eq!(fetched["price"].try_get::<i64>(), Some(42));
}
