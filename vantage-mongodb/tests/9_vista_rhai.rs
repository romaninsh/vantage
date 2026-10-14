//! Rhai scripting over MongoDB vistas, against a live MongoDB: `modify:`
//! scripts, `rhai:` pipeline views and scripted references. Set `MONGODB_URL`
//! (defaults to `mongodb://localhost:27017`). Each test runs against a fresh
//! randomised database and drops it on the way out.

#![cfg(feature = "rhai")]

use std::error::Error;
use std::sync::Arc;

use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_mongodb::MongoDB;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_types::Record;
use vantage_vista::{Capability, SpecResolver, VistaFactory, contract};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_rhai_{}", ObjectId::new().to_hex());
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

fn resolver(specs: Vec<MongoVistaSpec>) -> Arc<dyn SpecResolver<MongoVistaSpec>> {
    let map: IndexMap<String, MongoVistaSpec> =
        specs.into_iter().map(|s| (s.name.clone(), s)).collect();
    Arc::new(move |name: &str| map.get(name).cloned())
}

fn text(row: &Record<CborValue>, key: &str) -> Option<String> {
    match row.get(key) {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

async fn seed_products(db: &MongoDB) -> TestResult {
    let rows = [
        ("cake", 150),
        ("bread", 5),
        ("cake", 20),
        ("candy", 300),
        ("bread", 7),
        ("cake", 30),
    ];
    let docs = rows
        .into_iter()
        .map(|(category, price)| doc! { "_id": ObjectId::new(), "category": category, "price": price })
        .collect::<Vec<_>>();
    db.doc_collection("products").insert_many(docs).await?;
    Ok(())
}

const PRODUCT: &str = r#"
name: product
columns:
  _id: { type: object_id, flags: [id] }
  category: { type: string }
  price: { type: int }
mongo:
  collection: products
  modify: |
    self.with_condition(#{price: #{"$gt": 100}})
"#;

#[tokio::test]
async fn modify_script_adds_condition() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;

    let v = db.vista_factory().build_from_spec(spec(PRODUCT))?;
    let rows = v.list_values().await?;
    let mut categories: Vec<String> = rows.values().filter_map(|r| text(r, "category")).collect();
    categories.sort();
    assert_eq!(categories, vec!["cake", "candy"]);
    assert_eq!(v.count().await?, 2);
    // A condition narrows; the collection stays writable.
    assert!(v.capabilities().has(Capability::Insert));
    // The script's filter is recorded under its canonical JSON.
    assert_eq!(
        v.query().native,
        vec![r#"{"price":{"$gt":{"$numberLong":"100"}}}"#.to_string()]
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn modify_script_converts_oid_and_date() -> TestResult {
    let (db, name) = setup().await;
    let keep = ObjectId::new();
    db.doc_collection("events")
        .insert_many(vec![
            doc! { "_id": keep, "at": bson::DateTime::parse_rfc3339_str("2026-03-01T00:00:00Z")? },
            doc! { "_id": ObjectId::new(), "at": bson::DateTime::parse_rfc3339_str("2025-01-01T00:00:00Z")? },
        ])
        .await?;
    let yaml = format!(
        r#"
name: events
columns:
  _id: {{ type: object_id, flags: [id] }}
  at: {{ type: datetime }}
mongo:
  modify: |
    self.with_condition(#{{_id: mongo::oid("{hex}"), at: #{{"$gte": mongo::date("2026-01-01T00:00:00Z")}}}})
"#,
        hex = keep.to_hex()
    );
    let v = db.vista_factory().build_from_spec(spec(&yaml))?;
    let ids: Vec<String> = v.list_values().await?.into_keys().collect();
    assert_eq!(ids, vec![keep.to_hex()]);

    teardown(&db, &name).await;
    Ok(())
}

/// Each verb on `self` starts from the built vista, and the script's value is
/// the result: the discarded `cheap` handle (which would leave no row) must
/// not narrow the `pricey` one built from `self` after it.
#[tokio::test]
async fn modify_script_resolves_self_twice() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;
    let yaml = PLAIN_PRODUCT.to_string()
        + r#"  modify: |
    let cheap = self.where("price", "<", 10);
    let pricey = self.with_condition(#{price: #{"$gt": 100}});
    pricey.where("category", "cake")
"#;
    let v = db.vista_factory().build_from_spec(spec(&yaml))?;

    let all = db.vista_factory().build_from_spec(spec(PLAIN_PRODUCT))?;
    let expected: Vec<String> = all
        .list_values()
        .await?
        .into_iter()
        .filter(|(_, r)| {
            text(r, "category").as_deref() == Some("cake")
                && r.get("price") == Some(&CborValue::Integer(150.into()))
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(expected.len(), 1);
    let ids: Vec<String> = v.list_values().await?.into_keys().collect();
    assert_eq!(ids, expected);
    assert_eq!(v.query().conditions.len(), 1);
    assert_eq!(v.query().native.len(), 1);

    teardown(&db, &name).await;
    Ok(())
}

const BY_CATEGORY: &str = r#"
name: category_counts
columns:
  _id: { type: string, flags: [id] }
  n: { type: int }
mongo:
  collection: products
  rhai: |
    [
      #{"$match": #{category: args.cat}},
      #{"$group": #{_id: "$category", n: #{"$sum": 1}}},
    ]
"#;

#[tokio::test]
async fn rhai_pipeline_view_with_args() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;

    let mut s = spec(BY_CATEGORY);
    s.driver.mongo.as_mut().expect("mongo block").args = vec![("cat".into(), "cake".into())];
    let v = db.vista_factory().build_from_spec(s)?;
    let rows = v.list_values().await?;
    let keys: Vec<&str> = rows.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["cake"]);
    assert_eq!(rows["cake"].get("n"), Some(&CborValue::Integer(3.into())));

    // Same read-only capabilities as a YAML `pipeline:` view.
    let caps = v.capabilities();
    assert!(caps.has(Capability::Order) && caps.has(Capability::Cursor));
    assert!(!caps.has(Capability::Insert) && !caps.has(Capability::Update));
    assert!(!caps.has(Capability::Delete) && !caps.has(Capability::Subscribe));
    let preview = v.preview().query.expect("a rendered query");
    assert!(preview.contains("$group"), "preview: {preview}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn rhai_pipeline_stages_are_validated() -> TestResult {
    let (db, name) = setup().await;
    let yaml = r#"
name: broken_view
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: products
  rhai: |
    [#{"$match": #{}}, #{group: #{_id: "$a"}}]
"#;
    let Err(err) = db.vista_factory().build_from_spec(spec(yaml)) else {
        panic!("a stage without a `$` operator must be rejected");
    };
    let msg = err.to_string();
    assert!(msg.contains("stage: 1"), "error: {msg}");
    assert!(msg.contains("broken_view"), "error names the spec: {msg}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn rhai_and_pipeline_together_rejected() -> TestResult {
    let (db, name) = setup().await;
    let yaml = r#"
name: both
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: products
  pipeline:
    - $match: {}
  rhai: |
    [#{"$match": #{}}]
"#;
    let Err(err) = db.vista_factory().build_from_spec(spec(yaml)) else {
        panic!("`pipeline:` and `rhai:` together must be rejected");
    };
    let msg = err.to_string();
    assert!(
        msg.contains("pipeline") && msg.contains("rhai"),
        "error: {msg}"
    );

    teardown(&db, &name).await;
    Ok(())
}

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title] }
mongo:
  collection: clients
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo:
      rhai: |
        table("order").where("client_id", me._id).where("status", "open")
"#;

const ORDER: &str = r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: object_id }
  item: { type: string, flags: [title] }
  status: { type: string }
mongo:
  collection: orders
"#;

#[tokio::test]
async fn build_script_reference_resolves_target() -> TestResult {
    let (db, name) = setup().await;
    let (c1, c2) = (ObjectId::new(), ObjectId::new());
    db.doc_collection("clients")
        .insert_many(vec![
            doc! { "_id": c1, "name": "Marty" },
            doc! { "_id": c2, "name": "Doc" },
        ])
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": c1, "item": "skateboard", "status": "open" },
            doc! { "_id": "o2", "client_id": c1, "item": "almanac", "status": "closed" },
            doc! { "_id": "o3", "client_id": c2, "item": "flux capacitor", "status": "open" },
        ])
        .await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec]));
    let clients = factory.build_from_spec(client_spec)?;
    let reference = clients.reference("orders").expect("orders ref");
    assert!(reference.build_script.is_some());

    let marty = clients
        .get_value(c1.to_hex().as_str())
        .await?
        .expect("client c1");
    let orders = clients.get_ref("orders", Some(&marty))?;
    assert_eq!(orders.name(), "order");
    let rows = orders.list_values().await?;
    let items: Vec<String> = rows.values().filter_map(|r| text(r, "item")).collect();
    // The script's extra `status` condition proves it ran, not the FK path.
    assert_eq!(items, vec!["skateboard"]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn build_script_reference_refuses_with_condition_on_table() -> TestResult {
    let (db, name) = setup().await;
    let client = CLIENT.replace(
        r#"table("order").where("client_id", me._id).where("status", "open")"#,
        r#"table("order").with_condition(#{client_id: mongo::oid(row._id)})"#,
    );
    let (client_spec, order_spec) = (spec(&client), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec]));
    let clients = factory.build_from_spec(client_spec)?;

    let mut row = Record::new();
    row.insert("_id".to_string(), CborValue::Text(ObjectId::new().to_hex()));
    let err = clients
        .get_ref("orders", Some(&row))
        .map(drop)
        .expect_err("with_condition on table(...) must fail");
    assert!(
        err.to_string().contains(r#"verb: "with_condition""#),
        "{err}"
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn capabilities_advertise_traverse_for_scripted_refs() -> TestResult {
    let (db, name) = setup().await;
    let v = db.vista_factory().build_from_spec(spec(ORDER))?;
    assert!(v.capabilities().has(Capability::Traverse));
    // A pipeline view's rows reach other tables through its references too.
    let view = db.vista_factory().build_from_spec(spec(BY_CATEGORY))?;
    assert!(view.capabilities().has(Capability::Traverse));
    assert!(!view.capabilities().has(Capability::Insert));
    teardown(&db, &name).await;
    Ok(())
}

const PLAIN_PRODUCT: &str = r#"
name: product
columns:
  _id: { type: object_id, flags: [id] }
  category: { type: string }
  price: { type: int }
mongo:
  collection: products
"#;

const CATEGORY_GROUPS: &str = r#"
name: category_groups
columns:
  _id: { type: string, flags: [id] }
  n: { type: int }
mongo:
  collection: products
  pipeline:
    - $group: { _id: "$category", n: { $sum: 1 } }
references:
  products:
    table: product
    kind: has_many
    foreign_key: category
    mongo:
      rhai: |
        table("product").where("category", me._id)
"#;

#[tokio::test]
async fn scripted_ref_from_group_view_row() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;

    let (groups_spec, product_spec) = (spec(CATEGORY_GROUPS), spec(PLAIN_PRODUCT));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![groups_spec.clone(), product_spec]));
    let groups = factory.build_from_spec(groups_spec)?;
    let cake = groups.get_value("cake").await?.expect("cake group");
    let products = groups.get_ref("products", Some(&cake))?;
    assert_eq!(products.name(), "product");
    assert_eq!(products.count().await?, 3);
    // The target is the writable collection, not the view.
    assert!(products.capabilities().has(Capability::Insert));
    contract::check_advertised(&groups).await;

    teardown(&db, &name).await;
    Ok(())
}

/// A client whose `orders` reference script builds `table("nope")`.
fn client_targeting_nope() -> MongoVistaSpec {
    spec(&CLIENT.replace(
        r#"table("order").where("client_id", me._id).where("status", "open")"#,
        r#"table("nope")"#,
    ))
}

fn any_client_row() -> Record<CborValue> {
    let mut row = Record::new();
    row.insert("_id".to_string(), CborValue::Text(ObjectId::new().to_hex()));
    row
}

#[tokio::test]
async fn script_table_unknown_to_resolver_errors() -> TestResult {
    let (db, name) = setup().await;
    let client_spec = client_targeting_nope();
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), spec(ORDER)]));
    let clients = factory.build_from_spec(client_spec)?;
    let err = clients
        .get_ref("orders", Some(&any_client_row()))
        .map(drop)
        .expect_err("an unknown table must fail with a resolver attached");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown table") && msg.contains("nope"),
        "{msg}"
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn script_table_without_resolver_is_bare_collection() -> TestResult {
    let (db, name) = setup().await;
    db.doc_collection("nope")
        .insert_one(doc! { "_id": "n1", "anything": 1 })
        .await?;
    let clients = db
        .vista_factory()
        .build_from_spec(client_targeting_nope())?;
    let target = clients.get_ref("orders", Some(&any_client_row()))?;
    assert_eq!(target.name(), "nope");
    assert_eq!(target.count().await?, 1);
    // A bare collection reads documents as stored, every field included.
    let rows = target.list_values().await?;
    assert_eq!(
        rows["n1"].get("anything"),
        Some(&CborValue::Integer(1.into()))
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn script_where_on_bare_collection_without_resolver() -> TestResult {
    let (db, name) = setup().await;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": "c1", "item": "skateboard" },
            doc! { "_id": "o2", "client_id": "c2", "item": "almanac" },
        ])
        .await?;
    let clients = db.vista_factory().build_from_spec(spec(&CLIENT.replace(
        r#"table("order").where("client_id", me._id).where("status", "open")"#,
        r#"table("orders").where("client_id", me._id)"#,
    )))?;
    let mut row = Record::new();
    row.insert("_id".to_string(), CborValue::Text("c1".into()));
    let orders = clients.get_ref("orders", Some(&row))?;
    let rows = orders.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), vec!["o1"]);
    assert_eq!(text(&rows["o1"], "item").as_deref(), Some("skateboard"));

    teardown(&db, &name).await;
    Ok(())
}
