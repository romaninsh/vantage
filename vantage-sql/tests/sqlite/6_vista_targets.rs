//! A reference's `target` is the name a catalog resolves: a typed
//! relation's built table name, or a YAML reference's spec name. Traversal
//! through a `Catalog` then needs nothing else.

#![cfg(feature = "vista")]

use std::error::Error;
use std::sync::Arc;

use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::vista::SqliteVistaSpec;
use vantage_vista::{
    Catalog, JoinKey, ReferenceKind, SpecResolver, Vista, VistaFactory, VistaResolver,
};

use super::vista::{clients_table, orders_table, setup_clients_orders};

type TestResult = std::result::Result<(), Box<dyn Error>>;

/// `orders` with a `has_one` back to `client`.
fn orders_vista(db: &SqliteDB) -> vantage_core::Result<Vista> {
    let target_db = db.clone();
    let orders = orders_table(db.clone()).with_one("client", "client_id", move |_| {
        clients_table(target_db.clone())
    });
    db.vista_factory().from_table(orders)
}

#[tokio::test]
async fn typed_reference_targets_are_table_names() -> TestResult {
    let db = setup_clients_orders().await;
    let clients = db.vista_factory().from_table(clients_table(db.clone()))?;
    assert_eq!(clients.references()["orders"].target, "orders");
    assert_eq!(orders_vista(&db)?.references()["client"].target, "client");
    Ok(())
}

/// The spec name wins over the physical table it reads (`order_book` reads
/// the `orders` table).
#[tokio::test]
async fn yaml_reference_targets_are_spec_names() -> TestResult {
    let db = setup_clients_orders().await;
    let client: SqliteVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
references:
  orders: { table: order_book, kind: has_many, foreign_key: client_id }
"#,
    )?;
    let order_book: SqliteVistaSpec = serde_yaml_ng::from_str(
        r#"
name: order_book
sqlite: { table: orders }
columns:
  id: { type: string, flags: [id] }
  client_id: { type: string }
"#,
    )?;
    let specs: Arc<IndexMap<String, SqliteVistaSpec>> =
        Arc::new([("order_book".to_string(), order_book)].into());
    let resolver: Arc<dyn SpecResolver<SqliteVistaSpec>> =
        Arc::new(move |n: &str| specs.get(n).cloned());

    let clients = db
        .vista_factory()
        .with_resolver(resolver)
        .build_from_spec(client)?;
    assert_eq!(clients.references()["orders"].target, "order_book");
    Ok(())
}

/// A column-level `references:` is a has-one named after its column, keyed
/// by that column, and targets the spec name it gives.
#[tokio::test]
async fn column_references_reach_the_metadata_as_has_one() -> TestResult {
    let db = setup_clients_orders().await;
    let orders: SqliteVistaSpec = serde_yaml_ng::from_str(
        r#"
name: orders
columns:
  id: { type: string, flags: [id] }
  client_id: { type: string, references: client }
"#,
    )?;

    let orders = db.vista_factory().build_from_spec(orders)?;
    let reference = &orders.references()["client_id"];
    assert_eq!(reference.kind, ReferenceKind::HasOne);
    assert_eq!(reference.target, "client");
    assert_eq!(reference.keys, [JoinKey::to_target_id("client_id")]);
    Ok(())
}

/// `Reference::narrow(catalog.vista(&r.target)?, row)` walks both join
/// kinds.
#[tokio::test]
async fn catalog_traversal_resolves_each_target_by_name() -> TestResult {
    let db = setup_clients_orders().await;
    let (client_db, orders_db) = (db.clone(), db.clone());
    let catalog = Catalog::new()
        .with("client", move |_: &str| {
            client_db
                .vista_factory()
                .from_table(clients_table(client_db.clone()))
        })
        .with("orders", move |_: &str| orders_vista(&orders_db));

    let clients = catalog.vista("client")?;
    let alice = clients.get_value("alice").await?.expect("alice exists");
    let has_many = &clients.references()["orders"];
    let alice_orders = has_many.narrow(catalog.vista(&has_many.target)?, &alice)?;
    let mut ids: Vec<String> = alice_orders.list_values().await?.into_keys().collect();
    ids.sort();
    assert_eq!(ids, ["o1", "o2"]);

    let orders = catalog.vista("orders")?;
    let o3 = orders.get_value("o3").await?.expect("o3 exists");
    let has_one = &orders.references()["client"];
    let owner = has_one.narrow(catalog.vista(&has_one.target)?, &o3)?;
    let ids: Vec<String> = owner.list_values().await?.into_keys().collect();
    assert_eq!(ids, ["bob"]);
    Ok(())
}
