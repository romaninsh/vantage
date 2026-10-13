//! Dotted join columns (`client.name`) on YAML vistas, read through
//! `$lookup`, against a live MongoDB. Set `MONGODB_URL` (defaults to
//! `mongodb://localhost:27017`). Each test runs against a fresh randomised
//! database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;
use std::sync::Arc;

use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{AnyMongoType, MongoDB, MongoId};
use vantage_types::Record;
use vantage_vista::{Capability, FilterOp, SortDirection, SpecResolver, Vista, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_joins_{}", ObjectId::new().to_hex());
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

const COUNTRY: &str = r#"
name: country
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
mongo:
  collection: countries
"#;

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
  country_id: { type: string }
mongo:
  collection: clients
references:
  country:
    table: country
    kind: has_one
    foreign_key: country_id
"#;

const ORDER: &str = r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
  item: { type: string, flags: [title] }
  client.name: { type: string, flags: [searchable] }
  client.country.name: { type: string }
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#;

fn resolver() -> SpecResolver<MongoVistaSpec> {
    let map: IndexMap<String, MongoVistaSpec> = [COUNTRY, CLIENT, ORDER]
        .into_iter()
        .map(|y| {
            let s = spec(y);
            (s.name.clone(), s)
        })
        .collect();
    Arc::new(move |name: &str| map.get(name).cloned())
}

fn orders(db: &MongoDB) -> Vista {
    db.vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(ORDER))
        .expect("orders vista")
}

struct Ids {
    marty: ObjectId,
}

/// Marty (USA) has o1 and o2, Doc (Canada) has o3, o4 points at no client.
async fn seed(db: &MongoDB) -> std::result::Result<Ids, Box<dyn Error>> {
    let (usa, canada) = (ObjectId::new(), ObjectId::new());
    let (marty, doc_brown) = (ObjectId::new(), ObjectId::new());
    db.doc_collection("countries")
        .insert_many(vec![
            doc! { "_id": usa, "name": "USA" },
            doc! { "_id": canada, "name": "Canada" },
        ])
        .await?;
    db.doc_collection("clients")
        .insert_many(vec![
            doc! { "_id": marty, "name": "Marty", "country_id": usa },
            doc! { "_id": doc_brown, "name": "Doc", "country_id": canada },
        ])
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": marty, "item": "skateboard" },
            doc! { "_id": "o2", "client_id": marty, "item": "almanac" },
            doc! { "_id": "o3", "client_id": doc_brown, "item": "flux capacitor" },
            doc! { "_id": "o4", "client_id": ObjectId::new(), "item": "hoverboard" },
        ])
        .await?;
    Ok(Ids { marty })
}

fn text(row: &Record<CborValue>, key: &str) -> Option<String> {
    match row.get(key) {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

fn column(rows: &IndexMap<String, Record<CborValue>>, key: &str) -> Vec<(String, Option<String>)> {
    rows.iter()
        .map(|(id, r)| (id.clone(), text(r, key)))
        .collect()
}

fn pair(id: &str, v: Option<&str>) -> (String, Option<String>) {
    (id.to_string(), v.map(str::to_string))
}

#[tokio::test]
async fn dotted_column_reads_joined_value() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let rows = orders(&db).list_values().await?;
    assert_eq!(
        column(&rows, "client.name"),
        vec![
            pair("o1", Some("Marty")),
            pair("o2", Some("Marty")),
            pair("o3", Some("Doc")),
            pair("o4", None),
        ]
    );
    // The foreign key itself still reads, and no join scaffolding leaks.
    let o1 = &rows["o1"];
    assert!(text(o1, "client_id").is_some());
    assert!(o1.keys().all(|k| !k.starts_with("__")), "{o1:?}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_two_hops() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let rows = orders(&db).list_values().await?;
    assert_eq!(
        column(&rows, "client.country.name"),
        vec![
            pair("o1", Some("USA")),
            pair("o2", Some("USA")),
            pair("o3", Some("Canada")),
            pair("o4", None),
        ]
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_missing_target_is_null() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let vista = orders(&db);
    assert_eq!(vista.count().await?, 4);
    let o4 = vista
        .get_value("o4")
        .await?
        .expect("dangling order still reads");
    assert_eq!(text(&o4, "item").as_deref(), Some("hoverboard"));
    assert_eq!(text(&o4, "client.name"), None);
    assert_eq!(text(&o4, "client.country.name"), None);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_filter_and_order() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut marty = orders(&db);
    marty.add_condition("client.name", FilterOp::Eq, CborValue::Text("Marty".into()))?;
    let rows = marty.list_values().await?;
    let mut ids: Vec<&str> = rows.keys().map(String::as_str).collect();
    ids.sort();
    assert_eq!(ids, vec!["o1", "o2"]);
    assert_eq!(marty.count().await?, 2);

    let mut by_country = orders(&db);
    by_country.add_condition(
        "client.country.name",
        FilterOp::Eq,
        CborValue::Text("Canada".into()),
    )?;
    let rows = by_country.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), vec!["o3"]);

    let mut sorted = orders(&db);
    sorted.set_order(Some(("client.name", SortDirection::Descending)))?;
    let rows = sorted.list_values().await?;
    let names: Vec<Option<String>> = rows.values().map(|r| text(r, "client.name")).collect();
    assert_eq!(
        names,
        vec![
            Some("Marty".into()),
            Some("Marty".into()),
            Some("Doc".into()),
            None,
        ]
    );

    // Keyset paging seeks on the joined sort key.
    let mut paged = orders(&db);
    paged.set_order(Some(("client.name", SortDirection::Ascending)))?;
    let (first, token) = paged.fetch_cursor(None, 3).await?;
    let (second, token) = paged.fetch_cursor(token, 3).await?;
    assert!(token.is_none());
    let seen: Vec<&str> = first
        .rows
        .iter()
        .chain(&second.rows)
        .map(|(id, _)| id.as_str())
        .collect();
    // A missing join sorts first; equal names fall back to `_id`.
    assert_eq!(seen, vec!["o4", "o3", "o1", "o2"]);

    // Quicksearch covers a searchable joined column.
    let mut searched = orders(&db);
    searched.set_search(Some("doc"))?;
    let rows = searched.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), vec!["o3"]);
    assert_eq!(searched.count().await?, 1);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_aggregate_is_refused() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let table = db
        .vista_factory()
        .with_resolver(resolver())
        .table_from_spec(&spec(ORDER))?;
    let joined = table
        .get_column::<AnyMongoType>("client.name")
        .expect("joined column");
    for result in [
        table.get_sum(&joined).await,
        table.get_max(&joined).await,
        table.get_min(&joined).await,
    ] {
        let err = result.expect_err("aggregating a join is unsupported");
        assert!(err.is_unsupported(), "{err}");
    }
    // A stored column still aggregates.
    let item = table.get_column::<AnyMongoType>("item").expect("item");
    assert_eq!(
        table.get_max(&item).await?.value().clone(),
        bson::Bson::String("skateboard".into())
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_stripped_on_write() -> TestResult {
    let (db, name) = setup().await;
    let ids = seed(&db).await?;
    let vista = orders(&db);

    // On a vista, `client.name` in an insert payload is a nested has-one
    // insert, so the table-level write path is where the strip shows.
    let table = db
        .vista_factory()
        .with_resolver(resolver())
        .table_from_spec(&spec(ORDER))?;
    let mut fresh: Record<AnyMongoType> = Record::new();
    fresh.insert("client_id".into(), AnyMongoType::untyped(ids.marty.into()));
    fresh.insert("item".into(), AnyMongoType::untyped("plutonium".into()));
    fresh.insert("client.name".into(), AnyMongoType::untyped("Biff".into()));
    let inserted = table.insert_value(MongoId::from("o5"), &fresh).await?;
    assert_eq!(
        inserted.get("client.name").map(|v| v.value().clone()),
        Some(bson::Bson::String("Marty".into()))
    );
    let raw = db
        .doc_collection("orders")
        .find_one(doc! { "_id": "o5" })
        .await?
        .expect("stored o5");
    assert_eq!(
        raw.keys().collect::<Vec<_>>(),
        vec!["_id", "client_id", "item"],
        "{raw:?}"
    );

    // A full-record round trip carries the joined value back; it is dropped.
    let mut o1 = vista.get_value("o1").await?.expect("o1");
    o1.insert("item".into(), CborValue::Text("longboard".into()));
    vista.replace_value("o1", &o1).await?;
    let raw = db
        .doc_collection("orders")
        .find_one(doc! { "_id": "o1" })
        .await?
        .expect("stored o1");
    assert_eq!(raw.get_str("item")?, "longboard");
    assert!(!raw.contains_key("client.name") && !raw.contains_key("client"));

    // A patch naming the joined column is refused; one without it applies.
    let mut partial = Record::new();
    partial.insert("client.name".to_string(), CborValue::Text("Biff".into()));
    assert!(vista.patch_value("o1", &partial).await.is_err());
    let mut partial = Record::new();
    partial.insert("item".to_string(), CborValue::Text("hoverboard".into()));
    vista.patch_value("o1", &partial).await?;
    let raw = db
        .doc_collection("orders")
        .find_one(doc! { "_id": "o1" })
        .await?
        .expect("stored o1");
    assert_eq!(raw.get_str("item")?, "hoverboard");
    assert!(!raw.contains_key("client.name") && !raw.contains_key("client"));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_write_under_joined_condition_is_refused() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut marty = orders(&db);
    marty.add_condition("client.name", FilterOp::Eq, CborValue::Text("Marty".into()))?;
    let err = marty.delete("o1").await.expect_err("confined by a join");
    assert!(err.to_string().contains("joined column"), "{err}");
    assert!(err.is_unsupported(), "{err}");
    assert_eq!(
        db.doc_collection("orders").count_documents(doc! {}).await?,
        4
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_coexists_with_fk_field() -> TestResult {
    let (db, name) = setup().await;
    let ids = seed(&db).await?;
    db.doc_collection("tickets")
        .insert_one(doc! { "_id": "t1", "client": ids.marty })
        .await?;

    let tickets = db
        .vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(
            r#"
name: ticket
columns:
  _id: { type: string, flags: [id] }
  client: { type: string }
  note: { type: string }
  client.name: { type: string }
mongo:
  collection: tickets
references:
  client:
    table: client
    kind: has_one
    foreign_key: client
"#,
        ))?;
    let t1 = tickets.get_value("t1").await?.expect("t1");
    assert_eq!(text(&t1, "client"), Some(ids.marty.to_hex()));
    assert_eq!(text(&t1, "client.name").as_deref(), Some("Marty"));

    // Patching a plain field keeps the stored foreign key (and so the join).
    let mut partial = Record::new();
    partial.insert("note".to_string(), CborValue::Text("late".into()));
    let patched = tickets.patch_value("t1", &partial).await?;
    assert_eq!(text(&patched, "client.name").as_deref(), Some("Marty"));
    let raw = db
        .doc_collection("tickets")
        .find_one(doc! { "_id": "t1" })
        .await?
        .expect("stored t1");
    assert_eq!(raw.get("client"), Some(&bson::Bson::ObjectId(ids.marty)));
    assert_eq!(raw.get_str("note")?, "late");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_on_pipeline_view_is_rejected() -> TestResult {
    let (db, name) = setup().await;

    let err = db
        .vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(
            r#"
name: order_view
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
  client.name: { type: string }
mongo:
  collection: orders
  pipeline:
    - $match: { item: { $exists: true } }
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#,
        ))
        .map(drop)
        .expect_err("a view cannot join");
    assert!(err.to_string().contains("pipeline view"), "{err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_through_unknown_relation_is_rejected() -> TestResult {
    let (db, name) = setup().await;

    let result = db
        .vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(
            r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  owner.name: { type: string }
mongo:
  collection: orders
"#,
        ));
    let err = result.map(drop).expect_err("no `owner` relation");
    // "No references defined on table" / "Reference not found on table".
    assert!(
        err.to_string().to_lowercase().contains("reference"),
        "{err}"
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn joined_columns_are_for_collections_only() -> TestResult {
    let (db, name) = setup().await;

    assert!(orders(&db).column("client.name").is_some());
    let view = db.vista_factory().build_from_spec(spec(
        r#"
name: totals
columns:
  _id: { type: string, flags: [id] }
  client.name: { type: string }
mongo:
  collection: orders
  pipeline:
    - $group: { _id: "$client_id" }
"#,
    ));
    let Err(err) = view else {
        panic!("a pipeline view cannot join");
    };
    assert!(err.to_string().contains("joined (dotted) column"), "{err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn get_value_includes_joined_column() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let vista = orders(&db);
    let o3 = vista.get_value("o3").await?.expect("o3");
    assert_eq!(text(&o3, "client.name").as_deref(), Some("Doc"));
    assert_eq!(text(&o3, "client.country.name").as_deref(), Some("Canada"));

    // Rows re-read for live updates carry the join too. The test deployment
    // is a replica set, so change streams are available.
    use futures_util::StreamExt;
    assert!(vista.capabilities().has(Capability::Subscribe));
    let mut stream = vista.watch().await?;
    db.doc_collection("orders")
        .update_one(
            doc! { "_id": "o3" },
            doc! { "$set": { "item": "delorean" } },
        )
        .await?;
    match stream.next().await.expect("an event")? {
        vantage_vista::VistaChange::Updated { id, value } => {
            assert_eq!(id, "o3");
            assert_eq!(text(&value, "client.name").as_deref(), Some("Doc"));
        }
        other => panic!("unexpected change {other:?}"),
    }

    teardown(&db, &name).await;
    Ok(())
}
