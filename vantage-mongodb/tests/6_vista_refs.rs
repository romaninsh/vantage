//! YAML-vista reference traversal against a live MongoDB. Set `MONGODB_URL`
//! (defaults to `mongodb://localhost:27017`). Each test runs against a fresh
//! randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;
use std::sync::Arc;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_mongodb::MongoDB;
use vantage_mongodb::vista::{MongoSpecResolver, MongoVistaSpec};
use vantage_vista::VistaFactory;

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_refs_{}", ObjectId::new().to_hex());
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

fn resolver(specs: Vec<MongoVistaSpec>) -> MongoSpecResolver {
    let map: IndexMap<String, MongoVistaSpec> =
        specs.into_iter().map(|s| (s.name.clone(), s)).collect();
    Arc::new(move |name: &str| map.get(name).cloned())
}

fn spec(yaml: &str) -> MongoVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("spec yaml")
}

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
  city:
    type: string
    mongo:
      nested_path: address.city
mongo:
  collection: clients
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
"#;

const ORDER: &str = r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
  item: { type: string, flags: [title] }
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#;

async fn seed(db: &MongoDB) -> TestResult {
    db.collection::<bson::Document>("clients")
        .insert_many(vec![
            bson::doc! { "_id": "c1", "name": "Marty", "address": { "city": "Hill Valley" } },
            bson::doc! { "_id": "c2", "name": "Doc", "address": { "city": "Twin Pines" } },
        ])
        .await?;
    db.collection::<bson::Document>("orders")
        .insert_many(vec![
            bson::doc! { "_id": "o1", "client_id": "c1", "item": "skateboard" },
            bson::doc! { "_id": "o2", "client_id": "c1", "item": "almanac" },
            bson::doc! { "_id": "o3", "client_id": "c2", "item": "flux capacitor" },
        ])
        .await?;
    Ok(())
}

fn text(row: &vantage_types::Record<CborValue>, key: &str) -> Option<String> {
    match row.get(key) {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn yaml_references_traverse_has_one_and_has_many() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec.clone()]));

    let clients = factory.build_from_spec(client_spec)?;
    let marty = clients.get_value("c1").await?.expect("client c1");
    let orders = clients.get_ref("orders", &marty)?;
    let rows = orders.list_values().await?;
    let mut items: Vec<String> = rows.values().filter_map(|r| text(r, "item")).collect();
    items.sort();
    assert_eq!(items, vec!["almanac", "skateboard"]);

    let orders = factory.build_from_spec(order_spec)?;
    let o3 = orders.get_value("o3").await?.expect("order o3");
    let client = orders.get_ref("client", &o3)?;
    let rows = client.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        text(rows.values().next().unwrap(), "name").as_deref(),
        Some("Doc")
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn references_traverse_object_id_foreign_keys() -> TestResult {
    let (db, name) = setup().await;
    let (c1, c2) = (ObjectId::new(), ObjectId::new());
    db.doc_collection("clients")
        .insert_many(vec![
            bson::doc! { "_id": c1, "name": "Marty", "address": { "city": "Hill Valley" } },
            bson::doc! { "_id": c2, "name": "Doc", "address": { "city": "Twin Pines" } },
        ])
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            bson::doc! { "_id": "o1", "client_id": c1, "item": "skateboard" },
            bson::doc! { "_id": "o2", "client_id": c1, "item": "almanac" },
            bson::doc! { "_id": "o3", "client_id": c2, "item": "flux capacitor" },
        ])
        .await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec.clone()]));

    let clients = factory.build_from_spec(client_spec)?;
    let marty = clients
        .get_value(c1.to_hex().as_str())
        .await?
        .expect("client c1");
    let orders = clients.get_ref("orders", &marty)?;
    let rows = orders.list_values().await?;
    let mut items: Vec<String> = rows.values().filter_map(|r| text(r, "item")).collect();
    items.sort();
    assert_eq!(items, vec!["almanac", "skateboard"]);

    // Inserting through the traversed set writes the foreign key as an ObjectId.
    let mut hoverboard = vantage_types::Record::new();
    hoverboard.insert("item".to_string(), CborValue::Text("hoverboard".into()));
    let new_id = orders.insert_return_id_value(&hoverboard).await?;
    let stored = db
        .doc_collection("orders")
        .find_one(bson::doc! { "_id": ObjectId::parse_str(&new_id)? })
        .await?
        .expect("inserted order");
    assert_eq!(stored.get("client_id"), Some(&bson::Bson::ObjectId(c1)));

    let orders = factory.build_from_spec(order_spec)?;
    let o3 = orders.get_value("o3").await?.expect("order o3");
    let client = orders.get_ref("client", &o3)?;
    let rows = client.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        text(rows.values().next().unwrap(), "name").as_deref(),
        Some("Doc")
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn reference_without_resolver_targets_bare_collection() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    // The reference names the target by collection, since nothing resolves it.
    let clients = db.vista_factory().build_from_spec(spec(
        r#"
name: client
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
mongo:
  collection: clients
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
"#,
    ))?;
    let doc = clients.get_value("c2").await?.expect("client c2");
    let orders = clients.get_ref("orders", &doc)?;
    let rows = orders.list_values().await?;
    assert_eq!(rows.len(), 1);
    // Column-less passthrough: the stored document, every key as stored.
    let row = rows.values().next().unwrap();
    assert_eq!(text(row, "item").as_deref(), Some("flux capacitor"));
    assert_eq!(text(row, "client_id").as_deref(), Some("c2"));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn nested_reference_key_is_refused_at_build() -> TestResult {
    let (db, name) = setup().await;
    let nested_order = spec(
        r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string, mongo: { nested_path: refs.client } }
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#,
    );

    let err = db
        .vista_factory()
        .build_from_spec(nested_order.clone())
        .err()
        .expect("nested has_one key refused");
    assert!(err.to_string().contains("its own field name"), "{err}");

    // The client's has_many lands on the same nested key, on the target.
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![spec(CLIENT), nested_order]));
    let err = factory
        .build_from_spec(spec(CLIENT))
        .err()
        .expect("nested has_many key refused");
    assert!(err.to_string().contains("client_id"), "{err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn traversal_into_target_with_renamed_reference_key_errors() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;
    // The order's own reference to product keys on a renamed field; the
    // client's has_many into order is sound.
    let order = spec(
        &(ORDER.replace(
            "  item: { type: string, flags: [title] }\n",
            "  item: { type: string, flags: [title] }\n  product_id: { type: string, mongo: { field: productId } }\n",
        ) + "  product:\n    table: product\n    kind: has_one\n    foreign_key: product_id\n"),
    );
    let clients = db
        .vista_factory()
        .with_resolver(resolver(vec![spec(CLIENT), order]))
        .build_from_spec(spec(CLIENT))?;

    let row = clients.get_value("c1").await?.expect("client c1");
    let err = clients
        .get_ref("orders", &row)
        .err()
        .expect("traversal into the order must fail loudly");
    let msg = err.to_string();
    assert!(msg.contains("its own field name"), "{msg}");
    assert!(msg.contains("product_id"), "{msg}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn bare_has_one_target_is_found_by_id() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let orders = db.vista_factory().build_from_spec(spec(
        r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
mongo:
  collection: orders
references:
  client:
    table: clients
    kind: has_one
    foreign_key: client_id
"#,
    ))?;
    let o3 = orders.get_value("o3").await?.expect("order o3");
    let rows = orders.get_ref("client", &o3)?.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        text(rows.values().next().unwrap(), "name").as_deref(),
        Some("Doc")
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn writes_through_traversed_set_accept_ids_as_read() -> TestResult {
    let (db, name) = setup().await;
    let c1 = ObjectId::new();
    db.doc_collection("clients")
        .insert_one(bson::doc! { "_id": c1, "name": "Marty" })
        .await?;
    db.doc_collection("orders")
        .insert_one(bson::doc! { "_id": "o1", "client_id": c1, "item": "skateboard" })
        .await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec]));
    let clients = factory.build_from_spec(client_spec)?;
    let marty = clients
        .get_value(c1.to_hex().as_str())
        .await?
        .expect("client");
    let orders = clients.get_ref("orders", &marty)?;

    let stored_fk = || async {
        db.doc_collection("orders")
            .find_one(bson::doc! { "_id": "o1" })
            .await
            .map(|d| d.and_then(|d| d.get("client_id").cloned()))
    };

    // Replace and patch carry client_id exactly as it was read.
    let mut order = orders.get_value("o1").await?.expect("order o1");
    assert_eq!(text(&order, "client_id"), Some(c1.to_hex()));
    order.insert("item".into(), CborValue::Text("longboard".into()));
    orders.replace_value("o1", &order).await?;
    assert_eq!(stored_fk().await?, Some(bson::Bson::ObjectId(c1)));

    let mut partial = vantage_types::Record::new();
    partial.insert("client_id".to_string(), CborValue::Text(c1.to_hex()));
    partial.insert("item".to_string(), CborValue::Text("hoverboard".into()));
    orders.patch_value("o1", &partial).await?;
    assert_eq!(stored_fk().await?, Some(bson::Bson::ObjectId(c1)));

    // Insert with the foreign key prefilled from the parent's hex id.
    let mut fresh = vantage_types::Record::new();
    fresh.insert("client_id".to_string(), CborValue::Text(c1.to_hex()));
    fresh.insert("item".to_string(), CborValue::Text("almanac".into()));
    let id = orders.insert_return_id_value(&fresh).await?;
    let stored = db
        .doc_collection("orders")
        .find_one(bson::doc! { "_id": ObjectId::parse_str(&id)? })
        .await?
        .expect("inserted order");
    assert_eq!(stored.get("client_id"), Some(&bson::Bson::ObjectId(c1)));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn traversal_target_keeps_nested_paths() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec, order_spec.clone()]));

    let orders = factory.build_from_spec(order_spec)?;
    let o1 = orders.get_value("o1").await?.expect("order o1");
    let client = orders.get_ref("client", &o1)?;
    let rows = client.list_values().await?;
    assert_eq!(rows.len(), 1);
    let row = rows.values().next().unwrap();
    assert_eq!(text(row, "city").as_deref(), Some("Hill Valley"));
    assert!(row.get("address").is_none());

    // The target vista built by `get_ref_target` carries the paths too.
    let target = orders.get_ref_target("client")?;
    let all = target.list_values().await?;
    let mut cities: Vec<String> = all.values().filter_map(|r| text(r, "city")).collect();
    cities.sort();
    assert_eq!(cities, vec!["Hill Valley", "Twin Pines"]);

    teardown(&db, &name).await;
    Ok(())
}
