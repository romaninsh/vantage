//! Has-one and has-many traversal, bare targets, and nested paths on the
//! target.

use vantage_dataset::prelude::*;
use vantage_vista::VistaFactory;

use super::{CLIENT, ORDER, TestResult, resolver, seed, setup, spec, teardown, text};

#[tokio::test]
async fn yaml_references_traverse_has_one_and_has_many_and_keep_nested_paths() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec.clone()]));
    let clients = factory.build_from_spec(client_spec)?;
    let orders = factory.build_from_spec(order_spec)?;

    // has_many: Marty's orders.
    let marty = clients.get_value("c1").await?.expect("client c1");
    let rows = clients
        .get_ref("orders", Some(&marty))?
        .list_values()
        .await?;
    let mut items: Vec<String> = rows.values().filter_map(|r| text(r, "item")).collect();
    items.sort();
    assert_eq!(items, vec!["almanac", "skateboard"]);

    // has_one: o3's client, its city read from `address.city`.
    let o3 = orders.get_value("o3").await?.expect("order o3");
    let client = orders.get_ref("client", Some(&o3))?;
    assert!(client.is_narrowed());
    let rows = client.list_values().await?;
    assert_eq!(rows.len(), 1);
    let row = rows.values().next().unwrap();
    assert_eq!(text(row, "name").as_deref(), Some("Doc"));
    assert_eq!(text(row, "city").as_deref(), Some("Twin Pines"));
    assert!(row.get("address").is_none());

    // The bare target carries the paths too.
    let all = orders.get_ref("client", None)?.list_values().await?;
    let mut cities: Vec<String> = all.values().filter_map(|r| text(r, "city")).collect();
    cities.sort();
    assert_eq!(cities, vec!["Hill Valley", "Twin Pines"]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn references_without_a_resolver_target_the_bare_collection() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    // Each reference names its target by collection, since nothing resolves it.
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

    // has_many: the orders whose client_id is c2.
    let c2 = clients.get_value("c2").await?.expect("client c2");
    let c2_orders = clients.get_ref("orders", Some(&c2))?;
    assert!(c2_orders.is_narrowed());
    let rows = c2_orders.list_values().await?;
    assert_eq!(rows.len(), 1);
    // Column-less passthrough: the stored document, every key as stored.
    let row = rows.values().next().unwrap();
    assert_eq!(text(row, "item").as_deref(), Some("flux capacitor"));
    assert_eq!(text(row, "client_id").as_deref(), Some("c2"));

    // has_one: the client whose `_id` is o3's client_id.
    let o3 = orders.get_value("o3").await?.expect("order o3");
    let rows = orders.get_ref("client", Some(&o3))?.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        text(rows.values().next().unwrap(), "name").as_deref(),
        Some("Doc")
    );

    teardown(&db, &name).await;
    Ok(())
}
