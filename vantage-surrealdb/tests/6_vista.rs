//! Vista integration: typed `Table<SurrealDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running SurrealDB. Set `SURREALDB_URL` (defaults to
//! `cbor://root:root@localhost:8000/bakery/v2`). Each test runs in a fresh
//! randomised database so parallel runs don't trigger transaction conflicts.

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use uuid::Uuid;
use vantage_dataset::prelude::*;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::VistaFactory;

use surreal_client::SurrealConnection;

type TestResult = std::result::Result<(), Box<dyn Error>>;

/// Build a per-test DSN with an isolated database name. The base DSN comes
/// from `SURREALDB_URL` (root auth + host); each test substitutes its own
/// database to avoid transaction conflicts between parallel runs.
fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

async fn setup() -> (SurrealDB, String) {
    let database = format!("vista_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    let table = format!("product_{}", Uuid::new_v4().simple());
    (db, table)
}

fn product_table(db: SurrealDB, table_name: &str) -> Table<SurrealDB, EmptyEntity> {
    Table::<SurrealDB, EmptyEntity>::new(table_name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

async fn seed_row(
    db: &SurrealDB,
    table_name: &str,
    id: &str,
    name: &str,
    price: i64,
    is_deleted: bool,
) -> std::result::Result<Thing, Box<dyn Error>> {
    let record = format!("{}:{}", table_name, id);
    let name_owned = name.to_string();
    db.execute(&surreal_expr!(
        &format!(
            "CREATE {} SET name = {{}}, price = {{}}, is_deleted = {{}}",
            record
        ),
        name_owned,
        price,
        is_deleted
    ))
    .await?;
    Ok(Thing::new(table_name, id))
}

#[tokio::test]
async fn vista_lists_typed_surreal_as_cbor() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "beta", "Beta", 20, true).await?;

    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), table_name);
    assert_eq!(vista.get_id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);

    let alpha_key = format!("{}:alpha", table_name);
    let alpha = rows.get(&alpha_key).expect("row alpha");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    assert_eq!(alpha.get("price"), Some(&CborValue::Integer(10i64.into())));
    assert_eq!(alpha.get("is_deleted"), Some(&CborValue::Bool(false)));

    Ok(())
}

#[tokio::test]
async fn vista_get_value_by_id() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;

    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    // Bare id — shell prefixes with table name.
    let row = vista.get_value("alpha").await?.expect("found");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Alpha".to_string())));

    // Full record id also works.
    let full = format!("{}:alpha", table_name);
    let row2 = vista.get_value(&full).await?.expect("found");
    assert_eq!(
        row2.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );

    let missing = vista.get_value("nope").await?;
    assert!(missing.is_none());

    Ok(())
}

#[tokio::test]
async fn vista_count_with_eq_condition() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "a", "A", 10, false).await?;
    seed_row(&db, &table_name, "b", "B", 20, true).await?;
    seed_row(&db, &table_name, "c", "C", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.get_count().await?, 3);

    vista.add_condition_eq("is_deleted", CborValue::Bool(false))?;
    assert_eq!(vista.get_count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);

    Ok(())
}

#[tokio::test]
async fn vista_yaml_loads_table_and_columns() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "biff", "Biff", 99, false).await?;

    let yaml = format!(
        r#"
name: product_view
columns:
  id:
    type: thing
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
surreal:
  table: {}
"#,
        table_name
    );

    let vista = db.vista_factory().from_yaml(&yaml)?;

    assert_eq!(vista.name(), "product_view");
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Biff".to_string())));

    Ok(())
}

#[tokio::test]
async fn vista_writes_round_trip_via_cbor() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    let record: Record<CborValue> = vec![
        ("name".to_string(), CborValue::Text("Delta".into())),
        ("price".to_string(), CborValue::Integer(99i64.into())),
        ("is_deleted".to_string(), CborValue::Bool(false)),
    ]
    .into_iter()
    .collect();

    vista.insert_value("delta", &record).await?;

    let fetched = vista.get_value("delta").await?.expect("inserted");
    assert_eq!(fetched.get("name"), Some(&CborValue::Text("Delta".into())));
    assert_eq!(
        fetched.get("price"),
        Some(&CborValue::Integer(99i64.into()))
    );

    vista.delete("delta").await?;
    assert!(vista.get_value("delta").await?.is_none());

    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(caps.can_insert);
    assert!(caps.can_update);
    assert!(caps.can_delete);
    // A read-write SurrealDB Vista is watchable via LIVE queries.
    assert!(caps.can_subscribe);
    assert_eq!(vista.driver(), "surrealdb");

    Ok(())
}

#[tokio::test]
async fn vista_eq_condition_with_typed_value() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "a", "Match", 10, false).await?;
    seed_row(&db, &table_name, "b", "Other", 20, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    vista.add_condition_eq("name", CborValue::Text("Match".into()))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Match".into())));

    Ok(())
}

#[tokio::test]
async fn vista_unknown_field_eq_errors() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    let err = vista
        .add_condition_eq("nonexistent", CborValue::Bool(true))
        .expect_err("should reject unknown field");
    assert!(err.to_string().contains("nonexistent"));

    Ok(())
}

#[tokio::test]
async fn vista_add_order_filters_results_with_replace_semantics() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "beta", "Beta", 20, false).await?;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "gamma", "Gamma", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_order);

    vista.add_order("price", SortDirection::Ascending)?;
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

    vista.add_order("name", SortDirection::Descending)?;
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

    vista.clear_orders()?;
    let _rows = vista.list_values().await?;
    Ok(())
}

#[tokio::test]
async fn vista_add_search_uses_string_contains() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "beta", "Beta", 20, false).await?;
    seed_row(&db, &table_name, "gamma", "Gamma", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_search);

    vista.add_search("amma")?;
    let rows = vista.list_values().await?;
    assert_eq!(
        rows.len(),
        1,
        "only Gamma contains 'amma' (case-insensitive)"
    );

    vista.add_search("alpha")?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);

    vista.clear_search()?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    Ok(())
}

#[tokio::test]
async fn vista_fetch_page_offset_pagination() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, table_name) = setup().await;
    for (id, name) in [
        ("a", "Apple"),
        ("b", "Banana"),
        ("c", "Cherry"),
        ("d", "Date"),
        ("e", "Elder"),
    ] {
        seed_row(&db, &table_name, id, name, 1, false).await?;
    }

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_fetch_page);

    vista.set_page_size(2)?;
    vista.add_order("name", SortDirection::Ascending)?;

    let p1 = vista.fetch_page(1).await?;
    let names_p1: Vec<String> = p1
        .iter()
        .map(|(_, r)| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(names_p1, vec!["Apple".to_string(), "Banana".into()]);

    let p2 = vista.fetch_page(2).await?;
    let names_p2: Vec<String> = p2
        .iter()
        .map(|(_, r)| match r.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(names_p2, vec!["Cherry".to_string(), "Date".into()]);

    let p3 = vista.fetch_page(3).await?;
    assert_eq!(p3.len(), 1);
    Ok(())
}

#[tokio::test]
async fn vista_fetch_next_chains_pages_until_exhausted() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, table_name) = setup().await;
    for (id, name) in [("a", "Apple"), ("b", "Banana"), ("c", "Cherry")] {
        seed_row(&db, &table_name, id, name, 1, false).await?;
    }

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().can_fetch_next);

    vista.set_page_size(2)?;
    vista.add_order("name", SortDirection::Ascending)?;

    let (r1, tok1) = vista.fetch_next(None).await?;
    assert_eq!(r1.len(), 2);
    assert!(tok1.is_some());

    let (r2, tok2) = vista.fetch_next(tok1).await?;
    assert_eq!(r2.len(), 1);
    assert!(tok2.is_none());
    Ok(())
}

// ── Query-sourced vistas: `rhai:` source and `base:`/`inherit:` derivation ──

#[cfg(feature = "rhai")]
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
#[cfg(feature = "rhai")]
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

/// A vista whose source is a Rhai-built SELECT (not a physical table). The
/// script filters `price > 15`, so only two products surface, and the vista is
/// read-only.
#[cfg(feature = "rhai")]
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
    select().from("product").field("id").field("name").where(expr("price > 15"))
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;
    assert_eq!(vista.name(), "expensive_products");

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(!caps.can_insert, "rhai-sourced vista is read-only");
    assert!(!caps.can_update);
    assert!(!caps.can_delete);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2, "only products with price > 15");
    assert!(rows.contains_key("product:b"));
    assert!(rows.contains_key("product:c"));
    assert!(!rows.contains_key("product:a"));
    Ok(())
}

/// A derived vista: `base: client` resolves eagerly through the factory's
/// resolver, the base table's `select()` is seeded into the Rhai engine as
/// `base` (transform mode), and the listed columns are inherited from the base.
/// The query source makes it read-only.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_base_inherits_columns_and_transforms() -> TestResult {
    use std::sync::Arc;
    use vantage_surrealdb::vista::{SurrealSpecResolver, SurrealVistaSpec};

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
    base.where(expr("name = 'Alice'"))
"#;

    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(client_yaml)?;
    let vip_spec: SurrealVistaSpec = serde_yaml_ng::from_str(vip_yaml)?;

    let map: indexmap::IndexMap<String, SurrealVistaSpec> =
        [("client".to_string(), client_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: SurrealSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

    let vista = db
        .vista_factory()
        .with_resolver(resolver)
        .build_from_spec(vip_spec)?;

    // Query-sourced → read-only.
    let caps = vista.capabilities();
    assert!(
        !caps.can_insert,
        "derived (query-sourced) vista is read-only"
    );
    assert!(!caps.can_update);
    assert!(!caps.can_delete);

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
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_base_aggregates_into_declared_column() -> TestResult {
    use std::sync::Arc;
    use vantage_surrealdb::vista::{SurrealSpecResolver, SurrealVistaSpec};

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
    base.clear_fields().field("client_id").expression(expr("math::sum(total) AS total_due")).group_by(expr("client_id"))
"#;

    let orders_spec: SurrealVistaSpec = serde_yaml_ng::from_str(orders_yaml)?;
    let totals_spec: SurrealVistaSpec = serde_yaml_ng::from_str(totals_yaml)?;

    let map: indexmap::IndexMap<String, SurrealVistaSpec> =
        [("orders".to_string(), orders_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: SurrealSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

    let vista = db
        .vista_factory()
        .with_resolver(resolver)
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
