//! Windowed reads and single-document reads through the vista against a live
//! MongoDB. Set `MONGODB_URL` (defaults to `mongodb://localhost:27017`). Each
//! test runs against a fresh randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use futures_util::TryStreamExt;
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

#[tokio::test]
async fn vista_fetch_window_reads_offset_slice() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let vista = ordered_products(&db);
    assert!(vista.capabilities().has(Capability::Window));

    let page = vista.fetch_window(2, 3).await?;
    assert_eq!(
        names(page.rows.iter().map(|(_, r)| r)),
        vec!["item-2", "item-3", "item-4"]
    );

    let tail = vista.fetch_window(4, 10).await?;
    assert_eq!(
        names(tail.rows.iter().map(|(_, r)| r)),
        vec!["item-4", "item-5"]
    );

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
async fn vista_fetch_cursor_non_unique_sort_has_no_gaps() -> TestResult {
    let (db, name) = setup().await;
    // Three rows share price 2, one more shares price 1.
    let docs = [(0, 1), (1, 1), (2, 2), (3, 2), (4, 2), (5, 3), (6, 4)]
        .into_iter()
        .map(|(i, price)| {
            // Ids are not in name order, so the `_id` tiebreak is observable.
            bson::doc! { "_id": ObjectId::new(), "name": format!("item-{}", 6 - i), "price": price }
        })
        .collect::<Vec<_>>();
    db.doc_collection("products").insert_many(docs).await?;

    let vista = ordered_products(&db);
    let rows = drain(&vista, 8).await?;

    // The full list in one query under the same total order.
    let expected: Vec<(String, String)> = db
        .doc_collection("products")
        .find(bson::doc! {})
        .sort(bson::doc! { "price": 1, "_id": 1 })
        .await?
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .map(|d| {
            (
                d.get_object_id("_id").unwrap().to_hex(),
                d.get_str("name").unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(rows, expected);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_fetch_cursor_pages_over_missing_sort_field() -> TestResult {
    let (db, name) = setup().await;
    let docs = vec![
        bson::doc! { "_id": ObjectId::new(), "name": "a", "price": 2 },
        bson::doc! { "_id": ObjectId::new(), "name": "b" },
        bson::doc! { "_id": ObjectId::new(), "name": "c", "price": 1 },
        bson::doc! { "_id": ObjectId::new(), "name": "d", "price": bson::Bson::Null },
        bson::doc! { "_id": ObjectId::new(), "name": "e" },
        bson::doc! { "_id": ObjectId::new(), "name": "f", "price": 2 },
    ];
    db.doc_collection("products").insert_many(docs).await?;

    // Missing and null sort first ascending and last descending; the `_id`
    // tiebreak (insertion order) orders the ties.
    let cases = [
        (SortDirection::Ascending, ["b", "d", "e", "c", "a", "f"]),
        (SortDirection::Descending, ["a", "f", "c", "b", "d", "e"]),
    ];
    for (direction, expected) in cases {
        let mut vista = ordered_products(&db);
        vista.set_order(Some(("price", direction)))?;
        let rows = drain(&vista, 6).await?;
        assert_eq!(name_list(&rows), expected, "{direction:?}");
    }

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
async fn vista_fetch_cursor_pages_over_mixed_number_and_string_sort_field() -> TestResult {
    let (db, name) = setup().await;
    let docs = vec![
        bson::doc! { "_id": ObjectId::new(), "name": "n1", "price": 1 },
        bson::doc! { "_id": ObjectId::new(), "name": "sb", "price": "b" },
        bson::doc! { "_id": ObjectId::new(), "name": "n2", "price": 2.5 },
        bson::doc! { "_id": ObjectId::new(), "name": "sa", "price": "a" },
        bson::doc! { "_id": ObjectId::new(), "name": "n3", "price": 3_i64 },
    ];
    db.doc_collection("products").insert_many(docs).await?;

    let mut vista = ordered_products(&db);
    assert_eq!(
        name_list(&drain(&vista, 5).await?),
        ["n1", "n2", "n3", "sa", "sb"]
    );

    vista.set_order(Some(("price", SortDirection::Descending)))?;
    assert_eq!(
        name_list(&drain(&vista, 5).await?),
        ["sb", "sa", "n3", "n2", "n1"]
    );

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
