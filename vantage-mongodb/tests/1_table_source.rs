//! Integration tests for MongoDB TableSource.
//!
//! Requires a running MongoDB instance. Set MONGODB_URL env var or defaults
//! to mongodb://localhost:27017. Uses a randomised database name so tests
//! don't collide.

use bson::oid::ObjectId;
use bson::{Bson, doc};
use vantage_dataset::prelude::*;
use vantage_mongodb::MongoId;
use vantage_mongodb::{AnyMongoType, MongoCondition, MongoDB, MongoType};
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
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

    table.insert_value(id.clone(), &rec).await.unwrap();

    let values = table.list_values().await.unwrap();
    assert_eq!(values.len(), 1);
    assert!(values.contains_key(&id));

    let fetched = &values[&id];
    assert_eq!(fetched["name"].try_get::<String>(), Some("Cupcake".into()));
    assert_eq!(fetched["price"].try_get::<i64>(), Some(250));

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
    table.insert_value(id.clone(), &rec).await.unwrap();

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
            .insert_value(MongoId::from(bson::oid::ObjectId::new()), &rec)
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
            .insert_value(MongoId::from(bson::oid::ObjectId::new()), &rec)
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
    table.insert_value(id.clone(), &rec).await.unwrap();

    let replacement = record(&[
        ("name", AnyMongoType::new("New".to_string())),
        ("price", AnyMongoType::new(99i64)),
    ]);
    table.replace_value(id.clone(), &replacement).await.unwrap();

    let fetched = table
        .get_value(id.clone())
        .await
        .unwrap()
        .expect("row exists");
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
    table.insert_value(id.clone(), &rec).await.unwrap();

    // Patch only the price — name and calories should be untouched
    let patch = record(&[("price", AnyMongoType::new(75i64))]);
    table.patch_value(id.clone(), &patch).await.unwrap();

    let fetched = table
        .get_value(id.clone())
        .await
        .unwrap()
        .expect("row exists");
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
    table.insert_value(id.clone(), &rec).await.unwrap();
    assert_eq!(table.get_count().await.unwrap(), 1);

    WritableValueSet::delete(&table, id.clone()).await.unwrap();
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
            .insert_value(MongoId::from(bson::oid::ObjectId::new()), &rec)
            .await
            .unwrap();
    }
    assert_eq!(table.get_count().await.unwrap(), 4);

    WritableValueSet::delete_all(&table).await.unwrap();
    assert_eq!(table.get_count().await.unwrap(), 0);

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
            .insert_value(MongoId::from(bson::oid::ObjectId::new()), &rec)
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
            .insert_value(MongoId::from(bson::oid::ObjectId::new()), &rec)
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
            .insert_value(MongoId::from(bson::oid::ObjectId::new()), &rec)
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

// ── Equality conditions and reference values ─────────────────────────

async fn sorted_ids(table: &Table<MongoDB, EmptyEntity>) -> Vec<String> {
    let mut ids: Vec<String> = table
        .list_values()
        .await
        .unwrap()
        .keys()
        .map(|id| id.to_string())
        .collect();
    ids.sort();
    ids
}

fn link_table(db: &MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::<MongoDB, EmptyEntity>::new("link", db.clone())
        .with_id_column("_id")
        .with_column_of::<AnyMongoType>("fk")
}

#[tokio::test]
async fn eq_value_condition_matches_an_id_in_either_spelling() {
    let (db, db_name) = setup().await;
    let oid = ObjectId::new();
    db.doc_collection("link")
        .insert_many(vec![
            doc! { "_id": "as_oid", "fk": oid },
            doc! { "_id": "as_hex", "fk": oid.to_hex() },
            doc! { "_id": "other", "fk": "c1" },
        ])
        .await
        .unwrap();

    for value in [Bson::ObjectId(oid), Bson::String(oid.to_hex())] {
        let condition = db
            .eq_value_condition("fk", AnyMongoType::untyped(value))
            .unwrap();
        let table = link_table(&db).with_condition(condition);
        assert_eq!(sorted_ids(&table).await, ["as_hex", "as_oid"]);
    }

    let plain = db
        .eq_value_condition("fk", AnyMongoType::untyped(Bson::String("c1".into())))
        .unwrap();
    assert_eq!(
        sorted_ids(&link_table(&db).with_condition(plain)).await,
        ["other"]
    );

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn coerce_reference_value_turns_a_hex_string_into_the_object_id_it_spells() {
    let (db, db_name) = setup().await;
    let oid = ObjectId::new();

    let coerced = db.coerce_reference_value(AnyMongoType::untyped(Bson::String(oid.to_hex())));
    assert_eq!(coerced.to_bson(), Bson::ObjectId(oid));

    let text = AnyMongoType::untyped(Bson::String("c1".into()));
    assert_eq!(
        db.coerce_reference_value(text).to_bson(),
        Bson::String("c1".into())
    );
    let number = AnyMongoType::untyped(Bson::Int64(7));
    assert_eq!(db.coerce_reference_value(number).to_bson(), Bson::Int64(7));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn an_equality_condition_fills_the_column_of_a_row_inserted_through_the_set() {
    let (db, db_name) = setup().await;
    let oid = ObjectId::new();
    let coll = db.doc_collection("link");

    let as_object_id = db.coerce_reference_value(AnyMongoType::untyped(Bson::String(oid.to_hex())));
    let by_id = db.eq_value_condition("fk", as_object_id).unwrap();
    link_table(&db)
        .with_condition(by_id)
        .insert_value(MongoId::from("n1"), &record(&[]))
        .await
        .unwrap();
    let stored = coll.find_one(doc! { "_id": "n1" }).await.unwrap().unwrap();
    assert_eq!(stored.get("fk"), Some(&Bson::ObjectId(oid)));

    let by_text = db
        .eq_value_condition("fk", AnyMongoType::untyped(Bson::String("c1".into())))
        .unwrap();
    link_table(&db)
        .with_condition(by_text)
        .insert_value(MongoId::from("n2"), &record(&[]))
        .await
        .unwrap();
    let stored = coll.find_one(doc! { "_id": "n2" }).await.unwrap().unwrap();
    assert_eq!(stored.get("fk"), Some(&Bson::String("c1".into())));

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn a_set_condition_that_is_no_equality_fills_nothing_and_refuses_rows_outside_it() {
    let (db, db_name) = setup().await;
    let table =
        link_table(&db).with_condition(MongoCondition::Doc(doc! { "fk": { "$in": ["a", "b"] } }));

    let err = table
        .insert_value(MongoId::from("n1"), &record(&[]))
        .await
        .expect_err("a row without fk is outside the set");
    assert!(err.is_conflict(), "{err}");

    table
        .insert_value(
            MongoId::from("n2"),
            &record(&[("fk", AnyMongoType::new("a".to_string()))]),
        )
        .await
        .unwrap();
    assert_eq!(sorted_ids(&table).await, ["n2"]);

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn condition_equality_reads_literal_equalities_and_nothing_else() {
    let (db, db_name) = setup().await;
    let read = |condition: MongoCondition| db.condition_equality(&condition);

    let plain = read(MongoCondition::Doc(doc! { "fk": "c1" })).unwrap();
    assert_eq!(plain.0, "fk");
    assert_eq!(plain.1.to_bson(), Bson::String("c1".into()));

    let explicit = read(MongoCondition::Doc(doc! { "n": { "$eq": 5_i64 } })).unwrap();
    assert_eq!(explicit.0, "n");
    assert_eq!(explicit.1.to_bson(), Bson::Int64(5));

    for not_equality in [
        doc! { "fk": { "$in": ["a", "b"] } },
        doc! { "n": { "$gt": 5 } },
        doc! { "a": 1, "b": 2 },
        doc! { "$or": [{ "a": 1 }, { "b": 2 }] },
        doc! { "tags": ["a", "b"] },
    ] {
        assert!(
            read(MongoCondition::Doc(not_equality.clone())).is_none(),
            "{not_equality}"
        );
    }
    assert!(read(MongoCondition::And(vec![doc! { "a": 1 }.into()])).is_none());

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn describe_condition_reads_as_the_expression() {
    let (db, db_name) = setup().await;
    let cond = db
        .eq_value_condition("parent", AnyMongoType::from("p1"))
        .unwrap();
    assert_eq!(db.describe_condition(&cond), "{ \"parent\": \"p1\" }");

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn condition_column_names_eq_and_in() {
    let (db, db_name) = setup().await;
    let eq = db
        .eq_value_condition("parent", AnyMongoType::from("p1"))
        .unwrap();
    assert_eq!(db.condition_column(&eq), Some("parent".to_string()));

    let invoices = Table::<MongoDB, EmptyEntity>::new("invoice", db.clone()).with_id_column("_id");
    let in_cond = db.related_in_condition("invoice_id", &invoices, "_id");
    assert_eq!(
        db.condition_column(&in_cond),
        Some("invoice_id".to_string())
    );
    assert_eq!(db.describe_condition(&in_cond), "invoice_id IN <deferred>");

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn in_values_condition_narrows_to_the_listed_values() {
    let (db, db_name) = setup().await;
    let oid = ObjectId::new();
    db.doc_collection("link")
        .insert_many(vec![
            doc! { "_id": "a", "fk": "a" },
            doc! { "_id": "b", "fk": "b" },
            doc! { "_id": "c", "fk": oid.to_hex() },
        ])
        .await
        .unwrap();

    let listed = |values: Vec<Bson>| {
        let values = values.into_iter().map(AnyMongoType::untyped).collect();
        link_table(&db).with_condition(db.in_values_condition("fk", values).unwrap())
    };
    let a_and_c = listed(vec![Bson::String("a".into()), Bson::ObjectId(oid)]);
    assert_eq!(sorted_ids(&a_and_c).await, ["a", "c"]);
    assert!(sorted_ids(&listed(vec![])).await.is_empty());

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn condition_column_names_an_in_values_condition() {
    let (db, db_name) = setup().await;
    let cond = db
        .in_values_condition("invoice_id", vec![AnyMongoType::from("i1")])
        .unwrap();
    assert_eq!(db.condition_column(&cond), Some("invoice_id".to_string()));
    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn condition_column_is_none_for_raw_expressions() {
    let (db, db_name) = setup().await;
    let or = MongoCondition::Doc(doc! { "$or": [{ "a": 1 }, { "b": 2 }] });
    assert_eq!(db.condition_column(&or), None);
    let two_fields = MongoCondition::Doc(doc! { "a": 1, "b": 2 });
    assert_eq!(db.condition_column(&two_fields), None);
    let and = MongoCondition::And(vec![doc! { "a": 1 }.into(), doc! { "b": 2 }.into()]);
    assert_eq!(db.condition_column(&and), None);

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn condition_column_names_a_comparison() {
    let (db, db_name) = setup().await;
    let gt = MongoCondition::Doc(doc! { "invoice_id": { "$gt": 1 } });
    assert_eq!(db.condition_column(&gt), Some("invoice_id".to_string()));
    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn condition_column_is_none_for_negations() {
    let (db, db_name) = setup().await;
    for negated in [
        doc! { "tenant_id": { "$ne": "acme" } },
        doc! { "tenant_id": { "$nin": ["acme"] } },
        doc! { "tenant_id": { "$not": { "$eq": "acme" } } },
        doc! { "tenant_id": { "$exists": false } },
    ] {
        let cond = MongoCondition::Doc(negated.clone());
        assert_eq!(db.condition_column(&cond), None, "{negated}");
    }
    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn related_in_collects_each_source_value_in_both_id_spellings() {
    let (db, db_name) = setup().await;
    let (a, b) = (ObjectId::new(), ObjectId::new());
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": a },
            doc! { "_id": "o2", "client_id": b.to_hex() },
        ])
        .await
        .unwrap();
    let orders = Table::<MongoDB, EmptyEntity>::new("orders", db.clone())
        .with_id_column("_id")
        .with_column_of::<AnyMongoType>("client_id");

    let resolved = db
        .related_in_condition("_id", &orders, "client_id")
        .resolve()
        .await
        .unwrap();
    let list = resolved
        .get_document("_id")
        .unwrap()
        .get_array("$in")
        .unwrap();
    for id in [a, b] {
        assert!(list.contains(&Bson::ObjectId(id)), "{id}: {list:?}");
        assert!(list.contains(&Bson::String(id.to_hex())), "{id}: {list:?}");
    }

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn related_in_refuses_a_source_column_stored_under_another_name() {
    let (db, db_name) = setup().await;
    for alias in ["refs.client", "clientId"] {
        let orders = Table::<MongoDB, EmptyEntity>::new("orders", db.clone())
            .with_column(Column::<AnyMongoType>::new("client_id").with_alias(alias));
        let err = db
            .related_in_condition("_id", &orders, "client_id")
            .resolve()
            .await
            .expect_err("a renamed reference key must be refused");
        assert!(
            err.to_string().contains("its own field name"),
            "{alias}: {err}"
        );
    }

    teardown(&db, &db_name).await;
}

// ── Search ───────────────────────────────────────────────────────────

fn people_table(db: &MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::<MongoDB, EmptyEntity>::new("people", db.clone())
        .with_id_column("_id")
        .with_column_of::<String>("name")
}

#[tokio::test]
async fn search_is_a_literal_case_insensitive_substring_match() {
    let (db, db_name) = setup().await;
    db.doc_collection("people")
        .insert_many(vec![
            doc! { "_id": "1", "name": "a.b" },
            doc! { "_id": "2", "name": "axb" },
            doc! { "_id": "3", "name": "Quick (fox)" },
        ])
        .await
        .unwrap();

    let people = people_table(&db);
    assert_eq!(sorted_ids(&people.clone().with_search("a.b")).await, ["1"]);
    assert_eq!(sorted_ids(&people.clone().with_search("A.B")).await, ["1"]);
    assert_eq!(sorted_ids(&people.clone().with_search("(fox")).await, ["3"]);
    assert_eq!(
        sorted_ids(&people.clone().with_search("nobody")).await,
        Vec::<String>::new()
    );

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn search_reads_each_column_at_its_stored_path() {
    let (db, db_name) = setup().await;
    db.doc_collection("people")
        .insert_many(vec![
            doc! { "_id": "1", "name": "zed", "address": { "city": "Hill Valley" } },
            doc! { "_id": "2", "name": "amy", "address": { "city": "Springfield" } },
        ])
        .await
        .unwrap();
    let people = people_table(&db)
        .with_column(Column::<AnyMongoType>::new("city").with_alias("address.city"));

    assert_eq!(sorted_ids(&people.clone().with_search("hill")).await, ["1"]);
    assert_eq!(sorted_ids(&people.clone().with_search("amy")).await, ["2"]);
    assert_eq!(
        sorted_ids(&people.clone().with_search("zzz")).await,
        Vec::<String>::new()
    );

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn search_covers_only_the_searchable_columns_once_any_is_flagged() {
    let (db, db_name) = setup().await;
    db.doc_collection("people")
        .insert_many(vec![
            doc! { "_id": "1", "name": "zed", "address": { "city": "Hill Valley" } },
            doc! { "_id": "2", "name": "amy", "address": { "city": "Springfield" } },
        ])
        .await
        .unwrap();
    let people = Table::<MongoDB, EmptyEntity>::new("people", db.clone())
        .with_id_column("_id")
        .with_column(Column::<AnyMongoType>::new("name").with_flag(ColumnFlag::Searchable))
        .with_column(Column::<AnyMongoType>::new("city").with_alias("address.city"));

    assert_eq!(sorted_ids(&people.clone().with_search("zed")).await, ["1"]);
    assert_eq!(
        sorted_ids(&people.clone().with_search("hill")).await,
        Vec::<String>::new()
    );

    teardown(&db, &db_name).await;
}

#[tokio::test]
async fn search_over_a_table_without_columns_matches_nothing() {
    let (db, db_name) = setup().await;
    db.doc_collection("people")
        .insert_one(doc! { "_id": "1", "name": "zed" })
        .await
        .unwrap();
    let bare = Table::<MongoDB, EmptyEntity>::new("people", db.clone());

    assert_eq!(sorted_ids(&bare.clone()).await, ["1"]);
    assert_eq!(
        sorted_ids(&bare.with_search("zed")).await,
        Vec::<String>::new()
    );

    teardown(&db, &db_name).await;
}
