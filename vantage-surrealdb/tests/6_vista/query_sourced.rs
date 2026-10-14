//! Query-sourced vistas: a `rhai:` source, a `modify:` script, and
//! `base:`/`inherit:` derivation. A query source makes a vista read-only.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::SurrealVistaSpec;
use vantage_vista::{Capability, SpecResolver, VistaFactory};

use super::{TestResult, seed_row, setup};

async fn seed_client(db: &SurrealDB, id: &str, name: &str) -> TestResult {
    let name_owned = name.to_string();
    db.execute(&surreal_expr!(
        &format!("CREATE client:{} SET name = {{}}", id),
        name_owned
    ))
    .await?;
    Ok(())
}

/// `client_id` is a record link (`Thing`) so a `GROUP BY client_id` aggregate
/// re-keys cleanly onto `client:<id>` — SurrealDB's native identity model.
async fn seed_order(db: &SurrealDB, id: &str, client: &str, total: i64) -> TestResult {
    db.execute(&surreal_expr!(
        &format!(
            "CREATE orders:{} SET client_id = client:{}, total = {{}}",
            id, client
        ),
        total
    ))
    .await?;
    Ok(())
}

/// A resolver that knows one spec, under `name`.
fn resolver(name: &str, spec: SurrealVistaSpec) -> Arc<dyn SpecResolver<SurrealVistaSpec>> {
    let map: IndexMap<String, SurrealVistaSpec> = [(name.to_string(), spec)].into_iter().collect();
    let map = Arc::new(map);
    Arc::new(move |name: &str| map.get(name).cloned())
}

/// A vista whose source is a Rhai-built SELECT (not a physical table). The
/// script filters `price > 15`, so only two products surface, and the vista is
/// read-only.
#[tokio::test]
async fn vista_yaml_rhai_source_is_read_only_and_filters() -> TestResult {
    let (db, _) = setup().await;
    seed_row(&db, "product", "a", "A", 10, false).await?;
    seed_row(&db, "product", "b", "B", 20, false).await?;
    seed_row(&db, "product", "c", "C", 30, false).await?;

    let yaml = r#"
name: expensive_products
columns:
  id:
    type: thing
    flags: [id]
  name:
    type: string
surreal:
  rhai: |
    surreal::select().from("product").field("id").field("name").where(surreal::expr("price > 15"))
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;
    assert_eq!(vista.name(), "expensive_products");

    let caps = vista.capabilities();
    assert!(
        !caps.has(Capability::Insert),
        "rhai-sourced vista is read-only"
    );
    assert!(!caps.has(Capability::Update));
    assert!(!caps.has(Capability::Delete));
    assert!(!caps.has(Capability::Subscribe));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2, "only products with price > 15");
    assert!(rows.contains_key("product:b"));
    assert!(rows.contains_key("product:c"));
    assert!(!rows.contains_key("product:a"));
    Ok(())
}

/// Each verb on `self` starts from the built vista, and the script's value is
/// the result: the discarded `cheap` handle (which would leave no row) must
/// not narrow the `live` one built from `self` after it.
#[tokio::test]
async fn modify_script_resolves_self_twice() -> TestResult {
    let (db, _) = setup().await;
    seed_row(&db, "product", "a", "A", 10, false).await?;
    seed_row(&db, "product", "b", "B", 20, false).await?;
    seed_row(&db, "product", "c", "C", 30, true).await?;

    let yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
  price: { type: int }
  is_deleted: { type: bool }
surreal:
  modify: |
    let cheap = self.where("price", "<", 15);
    let live = self.with_condition(surreal::ident("is_deleted") == false);
    live.where("price", ">", 15)
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;
    let ids: Vec<String> = vista.list_values().await?.into_keys().collect();
    assert_eq!(ids, vec!["product:b"]);
    assert_eq!(vista.query().conditions.len(), 1);
    assert_eq!(vista.query().native.len(), 1);
    Ok(())
}

/// A derived vista: `base: client` resolves eagerly through the factory's
/// resolver, the base table's `select()` is seeded into the Rhai engine as
/// `base` (transform mode), and the listed columns are inherited from the base.
/// The query source makes it read-only.
#[tokio::test]
async fn vista_yaml_base_inherits_columns_and_transforms() -> TestResult {
    let (db, _) = setup().await;
    seed_client(&db, "alice", "Alice").await?;
    seed_client(&db, "bob", "Bob").await?;

    let client_yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
"#;
    let vip_yaml = r#"
name: vip_clients
columns: {}
surreal:
  base: client
  inherit:
    columns: [id, name]
  rhai: |
    base.where(surreal::expr("name = 'Alice'"))
"#;

    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(client_yaml)?;
    let vip_spec: SurrealVistaSpec = serde_yaml_ng::from_str(vip_yaml)?;

    let vista = db
        .vista_factory()
        .with_resolver(resolver("client", client_spec))
        .build_from_spec(vip_spec)?;

    // Query-sourced → read-only.
    let caps = vista.capabilities();
    assert!(
        !caps.has(Capability::Insert),
        "derived (query-sourced) vista is read-only"
    );
    assert!(!caps.has(Capability::Update));
    assert!(!caps.has(Capability::Delete));

    // Transform applied (only Alice) and inherited columns surface.
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1, "rhai transform filtered to Alice");
    let alice = rows.get("client:alice").expect("alice present");
    assert_eq!(alice.get("name"), Some(&CborValue::Text("Alice".into())));
    Ok(())
}

/// The headline "derived aggregate" case: a `debtors`-style vista derived from
/// `orders`, grouping by client and summing totals into a declared `total_due`
/// column, re-keyed onto the `client_id` record link.
#[tokio::test]
async fn vista_yaml_base_aggregates_into_declared_column() -> TestResult {
    let (db, _) = setup().await;
    seed_order(&db, "o1", "alice", 10).await?;
    seed_order(&db, "o2", "alice", 20).await?;
    seed_order(&db, "o3", "bob", 30).await?;

    let orders_yaml = r#"
name: orders
columns:
  id: { type: thing, flags: [id] }
  client_id: { type: thing }
  total: { type: int }
"#;
    let totals_yaml = r#"
name: client_totals
id_column: client_id
columns:
  total_due: { type: int }
surreal:
  base: orders
  inherit:
    columns: [client_id]
  rhai: |
    base.clear_fields().field("client_id").expression(surreal::expr("math::sum(total) AS total_due")).group_by(surreal::expr("client_id"))
"#;

    let orders_spec: SurrealVistaSpec = serde_yaml_ng::from_str(orders_yaml)?;
    let totals_spec: SurrealVistaSpec = serde_yaml_ng::from_str(totals_yaml)?;

    let vista = db
        .vista_factory()
        .with_resolver(resolver("orders", orders_spec))
        .build_from_spec(totals_spec)?;

    // Re-keyed by client_id; one row per client with the summed total.
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2, "one row per client");
    let alice = rows.get("client:alice").expect("alice total");
    assert_eq!(
        alice.get("total_due"),
        Some(&CborValue::Integer(30i64.into()))
    );
    let bob = rows.get("client:bob").expect("bob total");
    assert_eq!(
        bob.get("total_due"),
        Some(&CborValue::Integer(30i64.into()))
    );
    Ok(())
}
