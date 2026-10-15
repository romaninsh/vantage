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
use vantage_mongodb::vista::{MongoBlock, MongoColumnBlock, MongoVistaSpec};
use vantage_mongodb::{AnyMongoType, MongoDB, MongoId};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, FilterOp, SortDirection, VistaFactory};

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
            id.clone(),
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
            id.clone(),
            &rec(&[("name", AnyMongoType::new("Tart".to_string()))]),
        )
        .await?;

    let vista = db.vista_factory().from_table(table)?;

    let row = vista.get_value(id.to_string()).await?.expect("found");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Tart".to_string())));

    let missing = vista.get_value("nonexistenthex000000000000").await?;
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
                MongoId::from(ObjectId::new()),
                &rec(&[
                    ("name", AnyMongoType::new(n.to_string())),
                    ("is_deleted", AnyMongoType::new(deleted)),
                ]),
            )
            .await?;
    }

    let mut vista = db.vista_factory().from_table(table)?;
    assert_eq!(vista.count().await?, 3);

    vista.add_condition("is_deleted", FilterOp::Eq, CborValue::Bool(false))?;
    assert_eq!(vista.count().await?, 2);

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
    assert_eq!(vista.id_column(), Some("_id"));
    assert_eq!(vista.title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Marty".to_string())));
    assert_eq!(row.get("is_paying_client"), Some(&CborValue::Bool(true)));

    teardown(&db, &name).await;
    Ok(())
}

#[test]
fn yaml_spec_parses_the_collection_columns_types_and_flags() {
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
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(spec.name, "client");
    assert_eq!(spec.columns.len(), 3);
    assert_eq!(
        spec.driver
            .mongo
            .as_ref()
            .and_then(|m| m.collection.as_deref()),
        Some("clients")
    );
    assert_eq!(spec.columns["_id"].col_type.as_deref(), Some("object_id"));
    assert!(spec.columns["name"].has_flag(vantage_vista::ColumnFlag::Title));
}

#[test]
fn yaml_mongo_blocks_reject_unknown_fields_naming_the_key() {
    let table = "name: c\ncolumns:\n  _id: { flags: [id] }\nmongo:\n  collection: c\n  bogus: 1\n";
    let err = serde_yaml_ng::from_str::<MongoVistaSpec>(table).unwrap_err();
    assert!(err.to_string().contains("bogus"), "{err}");

    let column = "name: c\ncolumns:\n  _id: { flags: [id], mongo: { bogus: 1 } }\n";
    let err = serde_yaml_ng::from_str::<MongoVistaSpec>(column).unwrap_err();
    assert!(err.to_string().contains("bogus"), "{err}");

    let reference = "name: c\ncolumns:\n  _id: { flags: [id] }\nreferences:\n  o:\n    table: o\n    kind: has_many\n    foreign_key: c_id\n    mongo: { bogus: 1 }\n";
    let err = serde_yaml_ng::from_str::<MongoVistaSpec>(reference).unwrap_err();
    assert!(err.to_string().contains("bogus"), "{err}");
}

#[test]
fn the_view_flag_parses_defaults_off_and_is_left_out_when_off() {
    let yaml = "name: report\ncolumns:\n  _id: { flags: [id] }\nmongo:\n  collection: sales_report\n  view: true\n";
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    assert!(spec.driver.mongo.as_ref().unwrap().view);
    assert!(!MongoBlock::default().view);

    let off = serde_yaml_ng::to_string(&MongoBlock::default()).unwrap();
    assert!(!off.contains("view"), "{off}");
    let on = MongoBlock {
        view: true,
        ..MongoBlock::default()
    };
    assert!(
        serde_yaml_ng::to_string(&on)
            .unwrap()
            .contains("view: true")
    );
}

#[test]
fn yaml_without_a_mongo_block_has_none_and_scripts_and_reference_blocks_parse() {
    let bare: MongoVistaSpec = serde_yaml_ng::from_str(
        "name: clients\ncolumns:\n  _id: { type: object_id, flags: [id] }\n",
    )
    .unwrap();
    assert!(bare.driver.mongo.is_none());

    let yaml = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
mongo:
  rhai: '[#{"$match": #{}}]'
  modify: 'self.with_condition(#{a: 1})'
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo:
      rhai: 'table("order").where("client_id", row._id)'
"#;
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let block = spec.driver.mongo.as_ref().unwrap();
    assert!(block.rhai.as_deref().unwrap().contains("$match"));
    assert!(block.modify.as_deref().unwrap().contains("with_condition"));
    assert!(block.args.is_empty());
    let reference = spec.references["orders"].driver.mongo.as_ref().unwrap();
    assert!(reference.rhai.as_deref().unwrap().starts_with("table("));
}

#[tokio::test]
async fn a_spec_without_a_mongo_block_reads_the_collection_named_like_the_spec() -> TestResult {
    let (db, name) = setup().await;
    db.doc_collection("clients")
        .insert_one(bson::doc! { "_id": "c1", "name": "Marty" })
        .await?;

    let vista = db.vista_factory().from_yaml(
        "name: clients\ncolumns:\n  _id: { type: string, flags: [id] }\n  name: { type: string }\n",
    )?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows["c1"].get("name"),
        Some(&CborValue::Text("Marty".into()))
    );

    teardown(&db, &name).await;
    Ok(())
}

#[test]
fn a_column_path_is_the_nested_path_split_on_dots_the_field_or_nothing() {
    let nested = MongoColumnBlock {
        field: None,
        nested_path: Some("address.city".into()),
    };
    assert_eq!(
        nested.resolved_path("city").unwrap(),
        Some(vec!["address".to_string(), "city".to_string()])
    );

    let renamed = MongoColumnBlock {
        field: Some("fullName".into()),
        nested_path: None,
    };
    assert_eq!(
        renamed.resolved_path("full_name").unwrap(),
        Some(vec!["fullName".to_string()])
    );

    assert_eq!(
        MongoColumnBlock::default()
            .resolved_path("anything")
            .unwrap(),
        None
    );
}

#[test]
fn a_nested_path_wins_over_a_field_when_both_are_set() {
    let both = MongoColumnBlock {
        field: Some("fullName".into()),
        nested_path: Some("name.full".into()),
    };
    assert_eq!(
        both.resolved_path("full_name").unwrap(),
        Some(vec!["name".to_string(), "full".to_string()])
    );
}

#[test]
fn an_empty_nested_path_or_empty_segment_is_refused_naming_the_column() {
    for bad in ["", "a..b", ".a", "a.", "."] {
        let block = MongoColumnBlock {
            field: None,
            nested_path: Some(bad.to_string()),
        };
        let err = block
            .resolved_path("city")
            .expect_err("a nested path with an empty segment must be refused");
        let msg = err.to_string();
        assert!(
            msg.contains("nested_path") && msg.contains("city"),
            "`{bad}`: {msg}"
        );
    }
}

#[test]
fn an_empty_field_is_refused_naming_the_column() {
    let block = MongoColumnBlock {
        field: Some(String::new()),
        nested_path: None,
    };
    let msg = block.resolved_path("name").unwrap_err().to_string();
    assert!(msg.contains("field") && msg.contains("name"), "{msg}");
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

    vista.insert_value(id.clone(), &record).await?;

    let fetched = vista.get_value(id.clone()).await?.expect("inserted");
    assert_eq!(fetched.get("name"), Some(&CborValue::Text("Pie".into())));
    assert_eq!(
        fetched.get("price"),
        Some(&CborValue::Integer(99i64.into()))
    );

    vista.delete(id.clone()).await?;
    assert!(vista.get_value(id.clone()).await?.is_none());

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, name) = setup().await;
    let table = Table::<MongoDB, EmptyEntity>::new("anything", db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Cursor,
        Capability::Traverse,
    ] {
        assert!(caps.has(capability), "{capability}");
    }
    let live = db.supports_change_streams();
    assert_eq!(caps.has(Capability::Subscribe), live);
    assert_eq!(caps.has(Capability::Import), live);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_insert_without_id_reads_back_the_stored_row() -> TestResult {
    let (db, name) = setup().await;
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let record: Record<CborValue> = [("name".to_string(), CborValue::Text("Scone".into()))]
        .into_iter()
        .collect();
    let id = vista.insert_return_id_value(&record).await?;
    assert_eq!(id.len(), 24, "a generated ObjectId in hex: {id}");
    let stored = vista.get_value(id.clone()).await?.expect("stored");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Scone".into())));

    // The quicksearch is part of the set: a row that misses it is refused
    // before it is stored.
    vista.set_search(Some("nothing like it"))?;
    let err = vista.insert_return_id_value(&record).await.unwrap_err();
    assert!(err.is_conflict(), "{err:?}");
    assert_eq!(vista.count().await?, 0);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_upsert_creates_then_replaces() -> TestResult {
    let (db, name) = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let id = ObjectId::new().to_hex();
    let named = |n: &str| -> Record<CborValue> {
        [("name".to_string(), CborValue::Text(n.into()))]
            .into_iter()
            .collect()
    };

    let created = vista.upsert_value(&id, &named("Bun")).await?;
    assert_eq!(created.get("name"), Some(&CborValue::Text("Bun".into())));
    let replaced = vista.upsert_value(&id, &named("Roll")).await?;
    assert_eq!(replaced.get("name"), Some(&CborValue::Text("Roll".into())));
    assert_eq!(vista.count().await?, 1);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_clone_owns_its_narrowing() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());
    for n in ["A", "B"] {
        table
            .insert_value(
                MongoId::from(ObjectId::new()),
                &rec(&[("name", AnyMongoType::new(n.to_string()))]),
            )
            .await?;
    }
    let vista = db.vista_factory().from_table(table)?;
    let narrowed =
        vista
            .clone()
            .with_condition("name", FilterOp::Eq, CborValue::Text("A".into()))?;
    assert_eq!(narrowed.count().await?, 1);
    assert_eq!(vista.count().await?, 2);
    assert!(narrowed.is_narrowed() && !vista.is_narrowed());

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
    vista.add_condition("zip", FilterOp::Eq, CborValue::Text("1985".into()))?;
    assert_eq!(vista.count().await?, 1);
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
    vista.insert_value(new_id.clone(), &record).await?;

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
    let read = vista.get_value(new_id.clone()).await?.expect("read");
    assert_eq!(
        read.get("full_name"),
        Some(&CborValue::Text("Biff Tannen".into()))
    );
    assert_eq!(read.get("zip"), Some(&CborValue::Text("2015".into())));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn a_nested_path_with_nothing_behind_it_reads_as_no_value() -> TestResult {
    let (db, name) = setup().await;
    db.collection::<bson::Document>("clients")
        .insert_many(vec![
            bson::doc! { "_id": "full", "address": { "city": "Riga" } },
            bson::doc! { "_id": "scalar", "address": "n/a" },
            bson::doc! { "_id": "missing" },
            bson::doc! { "_id": "no_city", "address": { "zip": "1001" } },
        ])
        .await?;
    let vista = db.vista_factory().from_yaml(
        r#"
name: client
columns:
  _id: { type: string, flags: [id] }
  city: { type: string, mongo: { nested_path: address.city } }
mongo:
  collection: clients
"#,
    )?;

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows["full"].get("city"),
        Some(&CborValue::Text("Riga".into()))
    );
    for id in ["scalar", "missing", "no_city"] {
        assert!(
            matches!(rows[id].get("city"), None | Some(CborValue::Null)),
            "{id}: {:?}",
            rows[id]
        );
    }

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_set_order_filters_results_with_replace_semantics() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());

    for (n, p) in [("Beta", 20i64), ("Alpha", 10), ("Gamma", 30)] {
        table
            .insert_value(
                MongoId::from(ObjectId::new()),
                &rec(&[
                    ("name", AnyMongoType::new(n.to_string())),
                    ("price", AnyMongoType::new(p)),
                ]),
            )
            .await?;
    }

    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().has(Capability::Order));

    vista.set_order(Some(("price", SortDirection::Ascending)))?;
    let rows = vista.list_values().await?;
    let names: Vec<String> = rows
        .values()
        .map(|r| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        names,
        vec!["Alpha".to_string(), "Beta".into(), "Gamma".into()]
    );

    vista.set_order(Some(("name", SortDirection::Descending)))?;
    let rows = vista.list_values().await?;
    let names: Vec<String> = rows
        .values()
        .map(|r| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        names,
        vec!["Gamma".to_string(), "Beta".into(), "Alpha".into()]
    );

    vista.set_order(None)?;
    let _rows = vista.list_values().await?;

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_set_search_uses_regex() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());

    for n in ["Alpha", "Beta", "Gamma"] {
        table
            .insert_value(
                MongoId::from(ObjectId::new()),
                &rec(&[("name", AnyMongoType::new(n.to_string()))]),
            )
            .await?;
    }

    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().has(Capability::Search));

    vista.set_search(Some("amma"))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1, "only Gamma matches 'amma'");

    vista.set_search(Some("alpha"))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1, "only Alpha matches");

    vista.set_search(None)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);

    teardown(&db, &name).await;
    Ok(())
}

fn names(rows: &[(String, Record<CborValue>)]) -> Vec<String> {
    rows.iter()
        .map(|(_, r)| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect()
}

#[tokio::test]
async fn vista_fetch_window_pages_through_the_set() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());

    for n in ["A", "B", "C", "D", "E"] {
        table
            .insert_value(
                MongoId::from(ObjectId::new()),
                &rec(&[("name", AnyMongoType::new(n.to_string()))]),
            )
            .await?;
    }

    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().has(Capability::Window));
    vista.set_order(Some(("name", SortDirection::Ascending)))?;

    assert_eq!(names(&vista.fetch_window(0, 2).await?.rows), vec!["A", "B"]);
    assert_eq!(names(&vista.fetch_window(2, 2).await?.rows), vec!["C", "D"]);
    let last = vista.fetch_window(4, 2).await?;
    assert_eq!(last.rows.len(), 1, "partial last window");
    assert_eq!(last.total, None);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_fetch_cursor_chains_pages_until_exhausted() -> TestResult {
    let (db, name) = setup().await;
    let table = product_table(db.clone());

    for n in ["A", "B", "C"] {
        table
            .insert_value(
                MongoId::from(ObjectId::new()),
                &rec(&[("name", AnyMongoType::new(n.to_string()))]),
            )
            .await?;
    }

    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().has(Capability::Cursor));
    vista.set_order(Some(("name", SortDirection::Ascending)))?;

    let (p1, tok1) = vista.fetch_cursor(None, 2).await?;
    assert_eq!(names(&p1.rows), vec!["A", "B"]);
    assert!(tok1.is_some());

    let (p2, tok2) = vista.fetch_cursor(tok1, 2).await?;
    assert_eq!(names(&p2.rows), vec!["C"], "partial last page exhausts");
    assert!(tok2.is_none());

    let err = vista
        .fetch_cursor(Some(CborValue::Integer(2.into())), 2)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("invalid fetch_cursor token"),
        "{err}"
    );

    teardown(&db, &name).await;
    Ok(())
}

/// An `orders` collection whose `lines` field is an embedded BSON array,
/// surfaced as a contains-many relation. MongoDB stores it natively.
fn orders_contained_table(db: MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::<MongoDB, EmptyEntity>::new("orders", db)
        .with_id_column("_id")
        .with_column_of::<AnyMongoType>("lines")
        .with_contained_many(
            "lines",
            "lines",
            |db| {
                Table::new("lines", db)
                    .with_column_of::<String>("sku")
                    .with_column_of::<i64>("qty")
            },
            None,
        )
}

fn line(sku: &str, qty: i64) -> CborValue {
    CborValue::Map(vec![
        (CborValue::Text("sku".into()), CborValue::Text(sku.into())),
        (
            CborValue::Text("qty".into()),
            CborValue::Integer(qty.into()),
        ),
    ])
}

#[tokio::test]
async fn contained_array_round_trips_on_mongo() -> TestResult {
    let (db, name) = setup().await;
    let vista = db
        .vista_factory()
        .from_table(orders_contained_table(db.clone()))?;
    assert!(vista.reference("lines").unwrap().kind.is_contained());

    // Insert a document with a native embedded array.
    let mut order = Record::new();
    order.insert(
        "lines".to_string(),
        CborValue::Array(vec![line("a", 1), line("b", 2)]),
    );
    vista.insert_value("o1", &order).await?;

    // The embedded array reads back natively (not a string).
    let doc = vista.get_value("o1").await?.unwrap();
    assert!(matches!(doc.get("lines"), Some(CborValue::Array(_))));
    let lines = vista.get_ref("lines", Some(&doc))?;
    assert_eq!(lines.list_values().await?.len(), 2);
    // Without a parent document, the bare target: no records, no writes.
    let bare = vista.get_ref("lines", None)?;
    assert!(bare.list_values().await?.is_empty());
    assert!(!bare.capabilities().has(vantage_vista::Capability::Insert));

    // Add a line — eager writeback $sets the whole array.
    let mut new = Record::new();
    new.insert("sku".to_string(), CborValue::Text("c".into()));
    new.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let id = lines.insert_return_id_value(&new).await?;
    assert_eq!(id, "2");

    let doc2 = vista.get_value("o1").await?.unwrap();
    let CborValue::Array(stored) = doc2.get("lines").unwrap() else {
        panic!("lines should be a BSON array");
    };
    assert_eq!(stored.len(), 3);

    // Patch one line; confirm it persisted.
    let mut patch = Record::new();
    patch.insert("qty".to_string(), CborValue::Integer(99i64.into()));
    lines.patch_value("0", &patch).await?;
    let doc3 = vista.get_value("o1").await?.unwrap();
    let lines3 = vista.get_ref("lines", Some(&doc3))?;
    assert_eq!(
        lines3.get_value("0").await?.unwrap().get("qty"),
        Some(&CborValue::Integer(99i64.into()))
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn contained_from_yaml_round_trips_on_mongo() -> TestResult {
    let (db, name) = setup().await;
    db.collection::<bson::Document>("orders")
        .insert_one(bson::doc! { "_id": "o1", "lines": [ { "sku": "a", "qty": 1 } ] })
        .await?;

    let yaml = r#"
name: orders
columns:
  _id: { type: string, flags: [id] }
  lines: { type: string }
mongo:
  collection: orders
contained:
  lines:
    host_column: lines
    kind: contains_many
    columns:
      sku: { type: string }
      qty: { type: int }
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;
    assert!(vista.reference("lines").unwrap().kind.is_contained());

    let doc = vista.get_value("o1").await?.unwrap();
    let lines = vista.get_ref("lines", Some(&doc))?;
    assert_eq!(lines.list_values().await?.len(), 1);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("b".into()));
    line.insert("qty".to_string(), CborValue::Integer(2i64.into()));
    lines.insert_return_id_value(&line).await?;

    let doc2 = vista.get_value("o1").await?.unwrap();
    let lines2 = vista.get_ref("lines", Some(&doc2))?;
    assert_eq!(lines2.list_values().await?.len(), 2);

    teardown(&db, &name).await;
    Ok(())
}
