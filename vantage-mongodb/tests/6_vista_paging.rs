//! Windowed reads and single-document reads through the vista against a live
//! MongoDB. Set `MONGODB_URL` (defaults to `mongodb://localhost:27017`). Each
//! test runs against a fresh randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_mongodb::MongoDB;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_vista::{Capability, FilterOp, SortDirection, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_paging_{}", ObjectId::new().to_hex());
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

const PRODUCT: &str = r#"
name: product
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title] }
  price: { type: int }
mongo:
  collection: products
"#;

async fn seed(db: &MongoDB) -> TestResult {
    // Inserted out of price order so ordering cannot hold by coincidence.
    let docs = [3, 0, 5, 1, 4, 2]
        .into_iter()
        .map(|i| bson::doc! { "_id": ObjectId::new(), "name": format!("item-{i}"), "price": i })
        .collect::<Vec<_>>();
    db.doc_collection("products").insert_many(docs).await?;
    Ok(())
}

fn ordered_products(db: &MongoDB) -> vantage_vista::Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(PRODUCT).expect("spec yaml");
    let mut vista = db.vista_factory().build_from_spec(spec).expect("vista");
    vista
        .set_order(Some(("price", SortDirection::Ascending)))
        .expect("order");
    vista
}

fn names<'a>(rows: impl Iterator<Item = &'a vantage_types::Record<CborValue>>) -> Vec<String> {
    rows.filter_map(|r| match r.get("name") {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    })
    .collect()
}

/// Windows read one total order: `price`, then `_id` among ties. Ids are
/// created in name order and inserted shuffled, so insertion order can't stand
/// in for `_id` order. An ordered `list` reads that order too.
#[tokio::test]
async fn vista_fetch_window_reads_offset_slices_in_price_then_id_order() -> TestResult {
    let (db, name) = setup().await;
    let (a, b, c) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    let (d, e, f) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    db.doc_collection("products")
        .insert_many(vec![
            bson::doc! { "_id": e, "name": "e", "price": 2 },
            bson::doc! { "_id": c, "name": "c", "price": 1 },
            bson::doc! { "_id": b, "name": "b", "price": 0 },
            bson::doc! { "_id": f, "name": "f", "price": 2 },
            bson::doc! { "_id": d, "name": "d", "price": 1 },
            bson::doc! { "_id": a, "name": "a", "price": 0 },
        ])
        .await?;

    let vista = ordered_products(&db);
    assert!(vista.capabilities().has(Capability::Window));

    let page = vista.fetch_window(1, 2).await?;
    assert_eq!(names(page.rows.iter().map(|(_, r)| r)), vec!["b", "c"]);

    let tail = vista.fetch_window(4, 10).await?;
    assert_eq!(names(tail.rows.iter().map(|(_, r)| r)), vec!["e", "f"]);

    let listed = vista.list_values().await?;
    assert_eq!(names(listed.values()), vec!["a", "b", "c", "d", "e", "f"]);

    teardown(&db, &name).await;
    Ok(())
}

/// Every cursor page of two rows of `vista` in order, as (id, name) pairs.
/// Fails when the chain outlasts `max_pages`.
async fn drain(
    vista: &vantage_vista::Vista,
    max_pages: usize,
) -> std::result::Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut token = None;
    for _ in 0..max_pages {
        let (page, next) = vista.fetch_cursor(token, 2).await?;
        for (id, row) in &page.rows {
            out.extend(
                names(std::iter::once(row))
                    .into_iter()
                    .map(|n| (id.clone(), n)),
            );
        }
        match next {
            Some(t) => token = Some(t),
            None => return Ok(out),
        }
    }
    Err(format!("fetch_cursor chain exceeded {max_pages} pages").into())
}

fn name_list(rows: &[(String, String)]) -> Vec<&str> {
    rows.iter().map(|(_, n)| n.as_str()).collect()
}

#[tokio::test]
async fn vista_fetch_cursor_pages_over_missing_null_number_and_string_sort_values() -> TestResult {
    let (db, name) = setup().await;
    let docs = vec![
        bson::doc! { "_id": ObjectId::new(), "name": "two-a", "price": 2 },
        bson::doc! { "_id": ObjectId::new(), "name": "missing-a" },
        bson::doc! { "_id": ObjectId::new(), "name": "text-b", "price": "b" },
        bson::doc! { "_id": ObjectId::new(), "name": "one", "price": 1 },
        bson::doc! { "_id": ObjectId::new(), "name": "null", "price": bson::Bson::Null },
        bson::doc! { "_id": ObjectId::new(), "name": "two-and-a-half", "price": 2.5 },
        bson::doc! { "_id": ObjectId::new(), "name": "missing-b" },
        bson::doc! { "_id": ObjectId::new(), "name": "two-b", "price": 2 },
        bson::doc! { "_id": ObjectId::new(), "name": "text-a", "price": "a" },
        bson::doc! { "_id": ObjectId::new(), "name": "missing-c" },
        bson::doc! { "_id": ObjectId::new(), "name": "three", "price": 3_i64 },
    ];
    db.doc_collection("products").insert_many(docs).await?;

    // Missing and null sort below numbers, numbers below strings; ties follow `_id`.
    let mut vista = ordered_products(&db);
    assert_eq!(
        name_list(&drain(&vista, 6).await?),
        [
            "missing-a",
            "null",
            "missing-b",
            "missing-c",
            "one",
            "two-a",
            "two-b",
            "two-and-a-half",
            "three",
            "text-a",
            "text-b",
        ]
    );

    vista.set_order(Some(("price", SortDirection::Descending)))?;
    assert_eq!(
        name_list(&drain(&vista, 6).await?),
        [
            "text-b",
            "text-a",
            "three",
            "two-and-a-half",
            "two-a",
            "two-b",
            "one",
            "missing-a",
            "null",
            "missing-b",
            "missing-c",
        ]
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_fetch_cursor_pages_over_mixed_id_types() -> TestResult {
    let (db, name) = setup().await;
    // Strings sort before ObjectIds; a page ending on a string id must still
    // reach every ObjectId document.
    let docs = vec![
        bson::doc! { "_id": "s1", "name": "s1" },
        bson::doc! { "_id": ObjectId::new(), "name": "o1" },
        bson::doc! { "_id": "s2", "name": "s2" },
        bson::doc! { "_id": ObjectId::new(), "name": "o2" },
        bson::doc! { "_id": "s3", "name": "s3" },
        bson::doc! { "_id": ObjectId::new(), "name": "o3" },
    ];
    db.doc_collection("products").insert_many(docs).await?;

    let spec: MongoVistaSpec = serde_yaml_ng::from_str(PRODUCT).expect("spec yaml");
    let vista = db.vista_factory().build_from_spec(spec).expect("vista");
    let rows = drain(&vista, 6).await?;
    assert_eq!(name_list(&rows), ["s1", "s2", "s3", "o1", "o2", "o3"]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_fetch_cursor_seek_combines_with_search() -> TestResult {
    const SEARCHABLE: &str = r#"
name: product
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title, searchable] }
  price: { type: int }
mongo:
  collection: products
"#;
    let (db, name) = setup().await;
    let docs = [
        ("red-a", 1),
        ("blue-b", 2),
        ("red-c", 3),
        ("red-d", 4),
        ("blue-e", 5),
        ("red-f", 6),
    ]
    .into_iter()
    .map(|(n, price)| bson::doc! { "_id": ObjectId::new(), "name": n, "price": price })
    .collect::<Vec<_>>();
    db.doc_collection("products").insert_many(docs).await?;

    let spec: MongoVistaSpec = serde_yaml_ng::from_str(SEARCHABLE).expect("spec yaml");
    let mut vista = db.vista_factory().build_from_spec(spec).expect("vista");
    vista.set_order(Some(("price", SortDirection::Ascending)))?;
    vista.set_search(Some("red"))?;
    let rows = drain(&vista, 4).await?;
    assert_eq!(name_list(&rows), ["red-a", "red-c", "red-d", "red-f"]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_fetch_cursor_refuses_tokens_it_did_not_issue() -> TestResult {
    let (db, name) = setup().await;
    let vista = ordered_products(&db);

    let not_an_array = CborValue::Integer(2.into());
    let err = vista
        .fetch_cursor(Some(not_an_array), 2)
        .await
        .expect_err("not an array");
    assert!(
        err.to_string().contains("invalid fetch_cursor token"),
        "{err}"
    );

    let not_bytes = CborValue::Array(vec![CborValue::Null]);
    let err = vista
        .fetch_cursor(Some(not_bytes), 2)
        .await
        .expect_err("not bytes");
    assert!(
        err.to_string().contains("invalid fetch_cursor token"),
        "{err}"
    );

    let not_a_document = CborValue::Array(vec![CborValue::Bytes(vec![1, 2, 3])]);
    let err = vista
        .fetch_cursor(Some(not_a_document), 2)
        .await
        .expect_err("not a document");
    assert!(
        err.to_string().contains("invalid fetch_cursor token"),
        "{err}"
    );

    // The sort has two keys (price, then the `_id` tiebreak).
    let too_few_values = CborValue::Array(vec![]);
    let err = vista
        .fetch_cursor(Some(too_few_values), 2)
        .await
        .expect_err("too few values");
    assert!(
        err.to_string().contains("invalid fetch_cursor token"),
        "{err}"
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_some_value_reads_one_document() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let vista = ordered_products(&db);
    let (id, record) = vista.get_some_value().await?.expect("a document");
    assert_eq!(names(std::iter::once(&record)), vec!["item-0"]);
    assert!(!id.is_empty());

    let mut descending = ordered_products(&db);
    descending.set_order(Some(("price", SortDirection::Descending)))?;
    let (_, record) = descending.get_some_value().await?.expect("a document");
    assert_eq!(names(std::iter::once(&record)), vec!["item-5"]);

    let mut narrowed = ordered_products(&db);
    narrowed.add_condition("price", FilterOp::Gte, CborValue::Integer(2.into()))?;
    let (_, record) = narrowed.get_some_value().await?.expect("a document");
    assert_eq!(names(std::iter::once(&record)), vec!["item-2"]);

    teardown(&db, &name).await;
    Ok(())
}
