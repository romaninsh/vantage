//! Has-one and has-many traversal, bare targets, and nested paths on the
//! target.

use vantage_dataset::prelude::*;
use vantage_vista::VistaFactory;

use super::{CLIENT, ORDER, TestResult, resolver, seed, setup, spec, teardown, text};

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
    let orders = clients.get_ref("orders", Some(&marty))?;
    let rows = orders.list_values().await?;
    let mut items: Vec<String> = rows.values().filter_map(|r| text(r, "item")).collect();
    items.sort();
    assert_eq!(items, vec!["almanac", "skateboard"]);

    let orders = factory.build_from_spec(order_spec)?;
    let o3 = orders.get_value("o3").await?.expect("order o3");
    let client = orders.get_ref("client", Some(&o3))?;
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
    let orders = clients.get_ref("orders", Some(&doc))?;
    assert!(orders.is_narrowed());
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
    let rows = orders.get_ref("client", Some(&o3))?.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        text(rows.values().next().unwrap(), "name").as_deref(),
        Some("Doc")
    );

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
    let client = orders.get_ref("client", Some(&o1))?;
    assert!(client.is_narrowed());
    let rows = client.list_values().await?;
    assert_eq!(rows.len(), 1);
    let row = rows.values().next().unwrap();
    assert_eq!(text(row, "city").as_deref(), Some("Hill Valley"));
    assert!(row.get("address").is_none());

    // The bare target carries the paths too.
    let target = orders.get_ref("client", None)?;
    let all = target.list_values().await?;
    let mut cities: Vec<String> = all.values().filter_map(|r| text(r, "city")).collect();
    cities.sort();
    assert_eq!(cities, vec!["Hill Valley", "Twin Pines"]);

    teardown(&db, &name).await;
    Ok(())
}
