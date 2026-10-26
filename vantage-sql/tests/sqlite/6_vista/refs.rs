//! Reference traversal over the client/orders pair: `has_many` and
//! `has_one` targets, inserts through a narrowed target, computed columns
//! across the hop, and references a `base:`-derived vista declares itself.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::vista::SqliteVistaSpec;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record, entity};
use vantage_vista::{JoinKey, ReferenceKind, SpecResolver, VistaFactory};

use super::{TestResult, clients_table, orders_table, setup_clients_orders};

#[tokio::test]
async fn vista_get_ref_has_many_via_row() -> TestResult {
    let db = setup_clients_orders().await;
    let clients = db.vista_factory().from_table(clients_table(db.clone()))?;

    let (id, alice) = clients
        .clone()
        .with_id(CborValue::Text("alice".into()))?
        .get_some_value()
        .await?
        .expect("alice exists");
    assert_eq!(id, "alice");
    assert_eq!(
        alice.get("name"),
        Some(&CborValue::Text("Alice".to_string()))
    );

    let alice_orders = clients.get_ref("orders", Some(&alice))?;
    // The join is a recorded condition on the target.
    assert!(alice_orders.is_narrowed());
    assert_eq!(alice_orders.query().conditions[0].column, "client_id");
    let rows = alice_orders.list_values().await?;
    assert_eq!(rows.len(), 2, "alice has 2 orders");
    assert!(rows.contains_key("o1"));
    assert!(rows.contains_key("o2"));
    assert!(!rows.contains_key("o3"));

    // Without a row, the bare target: every order.
    let all_orders = clients.get_ref("orders", None)?;
    assert!(!all_orders.is_narrowed());
    assert_eq!(all_orders.count().await?, 3);
    Ok(())
}

/// A row inserted through a target narrowed to a parent gets the parent's
/// key in its foreign-key column.
#[tokio::test]
async fn vista_insert_into_has_many_target_fills_the_foreign_key() -> TestResult {
    let db = setup_clients_orders().await;
    let clients = db.vista_factory().from_table(clients_table(db.clone()))?;
    let alice = clients.get_value("alice").await?.expect("alice exists");
    let alice_orders = clients.get_ref("orders", Some(&alice))?;

    let order: Record<CborValue> = [("total".to_string(), CborValue::Integer(5i64.into()))]
        .into_iter()
        .collect();
    alice_orders.insert_value("o9", &order).await?;

    let all_orders = db.vista_factory().from_table(orders_table(db.clone()))?;
    let stored = all_orders.get_value("o9").await?.expect("inserted");
    assert_eq!(
        stored.get("client_id"),
        Some(&CborValue::Text("alice".into()))
    );
    assert_eq!(alice_orders.count().await?, 3);
    Ok(())
}

/// A `has_one` join key names the target's id column only once the target
/// is built, so traversal resolves it from the target.
#[tokio::test]
async fn vista_get_ref_has_one_narrows_by_the_target_id() -> TestResult {
    let db = setup_clients_orders().await;
    let target_db = db.clone();
    let orders = orders_table(db.clone()).with_one("client", "client_id", move |_| {
        clients_table(target_db.clone())
    });
    let orders = db.vista_factory().from_table(orders)?;

    let client = &orders.references()["client"];
    assert_eq!(client.kind, ReferenceKind::HasOne);
    assert_eq!(client.keys, [JoinKey::to_target_id("client_id")]);

    let o3 = orders.get_value("o3").await?.expect("o3 exists");
    let bob = orders.get_ref("client", Some(&o3))?;
    assert!(bob.is_narrowed());
    assert_eq!(bob.query().conditions[0].column, "id");
    let rows = bob.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["bob"]);

    assert_eq!(orders.get_ref("client", None)?.count().await?, 2);
    Ok(())
}

#[tokio::test]
async fn vista_get_ref_unknown_relation_is_an_error() -> TestResult {
    let db = setup_clients_orders().await;
    let clients = db.vista_factory().from_table(clients_table(db))?;
    let err = clients.get_ref("nope", None).unwrap_err();
    assert!(
        !err.is_unsupported(),
        "an unknown relation is a normal error"
    );
    assert!(err.is_not_found(), "{err}");
    Ok(())
}

/// Regression: `with_expression` columns must survive reference traversal.
/// `get_ref` resolves the child through `get_ref_from_row`, which erases the
/// entity to `EmptyEntity`. Before the fix, `Table::into_entity` dropped the
/// expression closures, so computed aggregates vanished from nested / drilldown
/// rows while still appearing on the top-level table — silently returning
/// fewer columns than the parent vista.
#[tokio::test]
async fn vista_get_ref_preserves_with_expression_columns() -> TestResult {
    #[entity(SqliteType)]
    #[derive(Debug, Clone, PartialEq, Default)]
    struct Order {
        client_id: String,
        total: i64,
    }

    let db = setup_clients_orders().await;

    let db_for_child = db.clone();
    let clients = Table::<SqliteDB, EmptyEntity>::new("client", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("orders", "client_id", move |_| {
            // A *typed* child (entity = Order, not EmptyEntity) carrying a
            // computed expression — get_ref will erase it to EmptyEntity.
            Table::<SqliteDB, Order>::new("orders", db_for_child.clone())
                .with_id_column("id")
                .with_column_of::<String>("client_id")
                .with_column_of::<i64>("total")
                .with_expression("total_with_tax", |_| sqlite_expr!("\"total\" * 2"))
        });

    let clients = db.vista_factory().from_table(clients)?;
    let (_, alice) = clients
        .clone()
        .with_id(CborValue::Text("alice".into()))?
        .get_some_value()
        .await?
        .expect("alice exists");

    let alice_orders = clients.get_ref("orders", Some(&alice))?;
    let rows = alice_orders.list_values().await?;
    assert_eq!(rows.len(), 2, "alice has 2 orders");
    let o1 = rows.get("o1").expect("order o1");
    assert_eq!(
        o1.get("total_with_tax"),
        Some(&CborValue::Integer(20i64.into())),
        "computed expression must survive get_ref entity erasure"
    );
    Ok(())
}

/// A `base:`-derived vista keeps the `references:` it declares itself, next
/// to whatever it inherits from the base (here: nothing).
#[tokio::test]
async fn vista_yaml_base_derived_declares_own_reference() -> TestResult {
    let db = setup_clients_orders().await;

    let client_yaml = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
"#;
    let orders_yaml = r#"
name: orders
columns:
  id: { type: string, flags: [id] }
  client_id: { type: string }
  total: { type: int }
"#;
    let derived_yaml = r#"
name: client_derived
columns: {}
sqlite:
  base: client
  inherit:
    columns: [id, name]
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
"#;

    let map: IndexMap<String, SqliteVistaSpec> = [
        ("client".to_string(), serde_yaml_ng::from_str(client_yaml)?),
        ("orders".to_string(), serde_yaml_ng::from_str(orders_yaml)?),
    ]
    .into_iter()
    .collect();
    let map = Arc::new(map);
    let resolver: Arc<dyn SpecResolver<SqliteVistaSpec>> =
        Arc::new(move |name: &str| map.get(name).cloned());

    let derived_spec: SqliteVistaSpec = serde_yaml_ng::from_str(derived_yaml)?;
    let derived = db
        .vista_factory()
        .with_resolver(resolver)
        .build_from_spec(derived_spec)?;

    let refs = derived.references();
    assert_eq!(refs.len(), 1);
    assert!(refs.contains_key("orders"));

    let alice = derived.get_value("alice").await?.expect("alice exists");
    let rows = derived
        .get_ref("orders", Some(&alice))?
        .list_values()
        .await?;
    let mut ids: Vec<&String> = rows.keys().collect();
    ids.sort();
    assert_eq!(ids, ["o1", "o2"]);
    Ok(())
}

const CLIENT_WITH_ORDERS: &str = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
"#;

fn resolver_of(specs: &[(&str, &str)]) -> Arc<dyn SpecResolver<SqliteVistaSpec>> {
    let map: IndexMap<String, SqliteVistaSpec> = specs
        .iter()
        .map(|(name, yaml)| (name.to_string(), serde_yaml_ng::from_str(yaml).unwrap()))
        .collect();
    let map = Arc::new(map);
    Arc::new(move |name: &str| map.get(name).cloned())
}

/// A resolver that has no spec for the target makes traversal an error, not
/// a column-less table over every row; the relation still shows.
#[tokio::test]
async fn reference_target_without_spec_is_an_error() -> TestResult {
    let db = setup_clients_orders().await;
    let clients = db
        .vista_factory()
        .with_resolver(resolver_of(&[]))
        .build_from_spec(serde_yaml_ng::from_str(CLIENT_WITH_ORDERS)?)?;
    assert!(clients.references().contains_key("orders"));

    let alice = clients.get_value("alice").await?.expect("alice exists");
    let Err(err) = clients.get_ref("orders", Some(&alice)) else {
        panic!("traversal fails");
    };
    assert!(
        format!("{err:?}").contains("reference target has no spec"),
        "{err:?}"
    );
    Ok(())
}

/// A target spec that fails to build makes traversal an error.
#[tokio::test]
async fn reference_target_failing_build_is_an_error() -> TestResult {
    let db = setup_clients_orders().await;
    let broken_orders = r#"
name: orders
columns: {}
sqlite:
  base: nowhere
"#;
    let clients = db
        .vista_factory()
        .with_resolver(resolver_of(&[("orders", broken_orders)]))
        .build_from_spec(serde_yaml_ng::from_str(CLIENT_WITH_ORDERS)?)?;
    let alice = clients.get_value("alice").await?.expect("alice exists");
    let Err(err) = clients.get_ref("orders", Some(&alice)) else {
        panic!("traversal fails");
    };
    assert!(
        format!("{err:?}").contains("reference target failed to build"),
        "{err:?}"
    );
    Ok(())
}
