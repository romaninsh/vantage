//! Vista integration: typed `Table<SqliteDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Uses in-memory SQLite — no external setup required.

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record, entity};
use vantage_vista::VistaFactory;

type TestResult = std::result::Result<(), Box<dyn Error>>;

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE product (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL,
            is_deleted INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO product VALUES \
         ('a', 'Alpha', 10, 0), \
         ('b', 'Beta', 20, 1), \
         ('c', 'Gamma', 30, 0)",
    )
    .execute(db.pool())
    .await
    .unwrap();

    db
}

fn product_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

#[tokio::test]
async fn vista_lists_typed_sqlite_as_cbor() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), "product");
    assert_eq!(vista.get_id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);

    let alpha = rows.get("a").expect("row a");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    assert_eq!(alpha.get("price"), Some(&CborValue::Integer(10i64.into())));
    Ok(())
}

#[tokio::test]
async fn vista_get_value_by_id() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let row = vista.get_value("b").await?.expect("row b exists");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Beta".to_string())));

    let missing = vista.get_value("nope").await?;
    assert!(missing.is_none());
    Ok(())
}

#[tokio::test]
async fn vista_count_with_eq_condition() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.get_count().await?, 3);

    vista.add_condition_eq("is_deleted", CborValue::Bool(false))?;
    assert_eq!(vista.get_count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a"));
    assert!(rows.contains_key("c"));
    assert!(!rows.contains_key("b"));
    Ok(())
}

#[tokio::test]
async fn vista_yaml_loads_table_and_columns() -> TestResult {
    let db = setup().await;

    let yaml = r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
sqlite:
  table: product
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;

    assert_eq!(vista.name(), "product_view");
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    assert!(rows.contains_key("a"));
    Ok(())
}

#[tokio::test]
async fn vista_writes_round_trip_via_cbor() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let record: Record<CborValue> = vec![
        ("name".to_string(), CborValue::Text("Delta".into())),
        ("price".to_string(), CborValue::Integer(99i64.into())),
        ("is_deleted".to_string(), CborValue::Bool(false)),
    ]
    .into_iter()
    .collect();

    vista.insert_value("d", &record).await?;

    let fetched = vista.get_value("d").await?.expect("inserted");
    assert_eq!(fetched.get("name"), Some(&CborValue::Text("Delta".into())));

    vista.delete("d").await?;
    assert!(vista.get_value("d").await?.is_none());
    Ok(())
}

/// Regression: `with_expression` survives `from_table`. Before the shell was
/// generic over `E`, the factory called `into_entity::<EmptyEntity>` which
/// reset the expressions map, silently dropping any computed columns.
#[tokio::test]
async fn vista_preserves_with_expression_columns() -> TestResult {
    let db = setup().await;

    #[entity(SqliteType)]
    #[derive(Debug, Clone, PartialEq, Default)]
    struct Product {
        name: String,
        price: i64,
    }

    let table = Table::<SqliteDB, Product>::new("product", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
        .with_expression("price_doubled", |_| sqlite_expr!("\"price\" * 2"));

    let vista = db.vista_factory().from_table(table)?;

    let rows = vista.list_values().await?;
    let alpha = rows.get("a").expect("row a");
    assert_eq!(
        alpha.get("price_doubled"),
        Some(&CborValue::Integer(20i64.into())),
        "computed column should appear in vista output"
    );
    let gamma = rows.get("c").expect("row c");
    assert_eq!(
        gamma.get("price_doubled"),
        Some(&CborValue::Integer(60i64.into()))
    );
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(caps.can_insert);
    assert!(caps.can_update);
    assert!(caps.can_delete);
    assert!(!caps.can_subscribe);
    Ok(())
}

/// A vista whose source is a Rhai-built SELECT (not a physical table). The
/// script filters `price > 15`, so only two products surface, and the vista is
/// read-only.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_rhai_source_is_read_only_and_filters() -> TestResult {
    let db = setup().await;

    let yaml = r#"
name: expensive_products
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
sqlite:
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
    assert!(rows.contains_key("b"));
    assert!(rows.contains_key("c"));
    assert!(!rows.contains_key("a"));
    Ok(())
}

async fn setup_clients_orders() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE client (id TEXT PRIMARY KEY, name TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE orders (id TEXT PRIMARY KEY, client_id TEXT NOT NULL, total INTEGER NOT NULL)",
    )
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO client VALUES ('alice','Alice'),('bob','Bob')")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO orders VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)")
        .execute(db.pool())
        .await
        .unwrap();
    db
}

fn orders_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("orders", db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
        .with_column_of::<i64>("total")
}

fn clients_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    let db_clone = db.clone();
    Table::<SqliteDB, EmptyEntity>::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("orders", "client_id", move |_| {
            orders_table(db_clone.clone())
        })
}

#[tokio::test]
async fn vista_get_ref_has_many_via_row() -> TestResult {
    let db = setup_clients_orders().await;
    let mut clients = db.vista_factory().from_table(clients_table(db.clone()))?;

    let (id, alice) = clients
        .with_id(CborValue::Text("alice".into()))?
        .get_some_value()
        .await?
        .expect("alice exists");
    assert_eq!(id, "alice");
    assert_eq!(
        alice.get("name"),
        Some(&CborValue::Text("Alice".to_string()))
    );

    let alice_orders = clients.get_ref("orders", &alice)?;
    let rows = alice_orders.list_values().await?;
    assert_eq!(rows.len(), 2, "alice has 2 orders");
    assert!(rows.contains_key("o1"));
    assert!(rows.contains_key("o2"));
    assert!(!rows.contains_key("o3"));
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

    let mut clients = db.vista_factory().from_table(clients)?;
    let (_, alice) = clients
        .with_id(CborValue::Text("alice".into()))?
        .get_some_value()
        .await?
        .expect("alice exists");

    let alice_orders = clients.get_ref("orders", &alice)?;
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

#[tokio::test]
async fn vista_list_references_surfaces_cardinality() -> TestResult {
    let db = setup_clients_orders().await;
    let clients = db.vista_factory().from_table(clients_table(db))?;

    let refs = clients.list_references();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].0, "orders");
    assert_eq!(refs[0].1, vantage_vista::ReferenceKind::HasMany);
    Ok(())
}

/// YAML-declared `references:` lower to `with_one`/`with_many`, with the
/// target table resolved by name through a spec resolver attached to the
/// factory. Mirrors the SurrealDB resolver pattern.
#[tokio::test]
async fn vista_yaml_references_resolve_via_resolver() -> TestResult {
    use indexmap::IndexMap;
    use std::sync::Arc;
    use vantage_sql::sqlite::vista::{SqliteSpecResolver, SqliteVistaSpec};

    let db = setup_clients_orders().await;

    let client_yaml = r#"
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
    let orders_yaml = r#"
name: orders
columns:
  id: { type: string, flags: [id] }
  client_id: { type: string }
  total: { type: int }
"#;

    let client_spec: SqliteVistaSpec = serde_yaml_ng::from_str(client_yaml)?;
    let orders_spec: SqliteVistaSpec = serde_yaml_ng::from_str(orders_yaml)?;

    let map: IndexMap<String, SqliteVistaSpec> =
        [("orders".to_string(), orders_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: SqliteSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

    let factory = db.vista_factory().with_resolver(resolver);
    let mut clients = factory.build_from_spec(client_spec)?;

    // Reference metadata surfaces from the wired relation.
    let refs = clients.list_references();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].0, "orders");
    assert_eq!(refs[0].1, vantage_vista::ReferenceKind::HasMany);

    // Traversal resolves `orders` via the resolver and filters to alice's rows.
    let (_id, alice) = clients
        .with_id(CborValue::Text("alice".into()))?
        .get_some_value()
        .await?
        .expect("alice exists");
    let alice_orders = clients.get_ref("orders", &alice)?;
    let rows = alice_orders.list_values().await?;
    assert_eq!(rows.len(), 2, "alice has 2 orders");
    assert!(rows.contains_key("o1"));
    assert!(rows.contains_key("o2"));
    assert!(!rows.contains_key("o3"));
    Ok(())
}

/// A derived vista: `base: client` resolves eagerly through the factory's
/// resolver, the base table's `select()` is seeded into the Rhai engine as
/// `base` (transform mode), and the listed columns are inherited from the
/// base. The query source makes it read-only.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_base_inherits_columns_and_transforms() -> TestResult {
    use indexmap::IndexMap;
    use std::sync::Arc;
    use vantage_sql::sqlite::vista::{SqliteSpecResolver, SqliteVistaSpec};

    let db = setup_clients_orders().await;

    let client_yaml = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
"#;
    let vip_yaml = r#"
name: vip_clients
columns: {}
sqlite:
  base: client
  inherit:
    columns: [id, name]
  rhai: |
    base.where(expr("name = 'Alice'"))
"#;

    let client_spec: SqliteVistaSpec = serde_yaml_ng::from_str(client_yaml)?;
    let vip_spec: SqliteVistaSpec = serde_yaml_ng::from_str(vip_yaml)?;

    let map: IndexMap<String, SqliteVistaSpec> =
        [("client".to_string(), client_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: SqliteSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

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
    let alice = rows.get("alice").expect("alice present");
    assert_eq!(alice.get("name"), Some(&CborValue::Text("Alice".into())));
    Ok(())
}

/// The headline "derived aggregate" case: a `debtors`-style vista derived from
/// `orders`, grouping by client and summing totals into a declared `total_due`
/// column, with the grouping key inherited and re-keyed via `id_column`.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_base_aggregates_into_declared_column() -> TestResult {
    use indexmap::IndexMap;
    use std::sync::Arc;
    use vantage_sql::sqlite::vista::{SqliteSpecResolver, SqliteVistaSpec};

    let db = setup_clients_orders().await;

    let orders_yaml = r#"
name: orders
columns:
  id: { type: string, flags: [id] }
  client_id: { type: string }
  total: { type: int }
"#;
    let totals_yaml = r#"
name: client_totals
id_column: client_id
columns:
  total_due: { type: int }
sqlite:
  base: orders
  inherit:
    columns: [client_id]
  rhai: |
    base.clear_fields().field("client_id").expression(expr("SUM(total) AS total_due")).group_by(expr("client_id"))
"#;

    let orders_spec: SqliteVistaSpec = serde_yaml_ng::from_str(orders_yaml)?;
    let totals_spec: SqliteVistaSpec = serde_yaml_ng::from_str(totals_yaml)?;

    let map: IndexMap<String, SqliteVistaSpec> =
        [("orders".to_string(), orders_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: SqliteSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

    let vista = db
        .vista_factory()
        .with_resolver(resolver)
        .build_from_spec(totals_spec)?;

    // Re-keyed by client_id; one row per client with the summed total.
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2, "one row per client");
    let alice = rows.get("alice").expect("alice total");
    assert_eq!(
        alice.get("total_due"),
        Some(&CborValue::Integer(30i64.into()))
    );
    let bob = rows.get("bob").expect("bob total");
    assert_eq!(
        bob.get("total_due"),
        Some(&CborValue::Integer(30i64.into()))
    );
    Ok(())
}

#[tokio::test]
async fn vista_add_order_ascending_descending_and_clear() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    // capability is advertised
    assert!(vista.capabilities().can_order);

    // sort ascending by price → a (10), b (20), c (30)
    vista.add_order("price", SortDirection::Ascending)?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a", "b", "c"]);

    // replace-semantics: switch to descending name → c (Gamma), b (Beta), a (Alpha)
    vista.add_order("name", SortDirection::Descending)?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["c", "b", "a"]);

    // clear → back to insertion order
    vista.clear_orders()?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a", "b", "c"]);
    Ok(())
}

#[tokio::test]
async fn vista_add_order_rejects_unknown_column() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    let result = vista.add_order("not_a_column", SortDirection::Ascending);
    assert!(result.is_err(), "unknown column must fail");
    Ok(())
}

#[tokio::test]
async fn vista_add_search_filters_results_with_replace_semantics() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert!(vista.capabilities().can_search);

    // search "alpha" → only row a (name = "Alpha")
    vista.add_search("alpha")?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a"], "search 'alpha' must match only row a");

    // replace-semantics: search "amma" → only row c (name = "Gamma")
    vista.add_search("amma")?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(
        ids,
        ["c"],
        "search 'amma' must match only row c after replace"
    );

    // clear → all rows back
    vista.clear_search()?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3, "clear_search must restore all rows");
    Ok(())
}

#[tokio::test]
async fn vista_clear_search_without_prior_search_is_noop() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    // No prior search — clear should silently succeed.
    vista.clear_search()?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    Ok(())
}

#[tokio::test]
async fn vista_fetch_page_offset_pagination() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert!(vista.capabilities().can_set_page_size);
    assert!(vista.capabilities().can_fetch_page);

    vista.set_page_size(2)?;
    vista.add_order("id", vantage_vista::SortDirection::Ascending)?;

    let page1 = vista.fetch_page(1).await?;
    let ids1: Vec<&String> = page1.iter().map(|(id, _)| id).collect();
    assert_eq!(ids1, ["a", "b"], "page 1 should be the first two rows");

    let page2 = vista.fetch_page(2).await?;
    let ids2: Vec<&String> = page2.iter().map(|(id, _)| id).collect();
    assert_eq!(ids2, ["c"], "page 2 should be the third row");

    let page3 = vista.fetch_page(3).await?;
    assert!(page3.is_empty(), "page 3 should be empty");
    Ok(())
}

#[tokio::test]
async fn vista_fetch_page_without_set_page_size_errors() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    // No set_page_size — fetch_page must reject loudly.
    let result = vista.fetch_page(1).await;
    assert!(
        result.is_err(),
        "fetch_page without set_page_size must error"
    );
    Ok(())
}

#[tokio::test]
async fn vista_set_page_size_zero_errors() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert!(vista.set_page_size(0).is_err(), "size 0 must reject");
    Ok(())
}

#[tokio::test]
async fn vista_fetch_page_honors_search_and_order() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.set_page_size(10)?;
    vista.add_order("name", SortDirection::Descending)?;
    vista.add_search("a")?; // matches Alpha, Beta, Gamma (all contain 'a' case-insensitively)

    let page = vista.fetch_page(1).await?;
    let names: Vec<String> = page
        .iter()
        .map(|(_, rec)| match rec.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        names,
        vec!["Gamma".to_string(), "Beta".to_string(), "Alpha".to_string()],
        "page must honour both search and DESC order on name"
    );
    Ok(())
}

#[tokio::test]
async fn vista_fetch_next_chains_pages_until_exhausted() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert!(vista.capabilities().can_fetch_next);

    vista.set_page_size(2)?;
    vista.add_order("id", SortDirection::Ascending)?;

    // First call — pass None.
    let (rows1, tok1) = vista.fetch_next(None).await?;
    let ids1: Vec<&String> = rows1.iter().map(|(id, _)| id).collect();
    assert_eq!(ids1, ["a", "b"], "first page");
    assert!(tok1.is_some(), "more pages available — token must be Some");

    // Second call — last partial page, 1 row → exhaustion signaled.
    let (rows2, tok2) = vista.fetch_next(tok1).await?;
    let ids2: Vec<&String> = rows2.iter().map(|(id, _)| id).collect();
    assert_eq!(ids2, ["c"], "last page");
    assert!(tok2.is_none(), "partial last page must exhaust");
    Ok(())
}

#[tokio::test]
async fn vista_fetch_next_resets_when_passed_none() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.set_page_size(2)?;
    vista.add_order("id", SortDirection::Ascending)?;

    // Walk to exhaustion.
    let (_p1, tok1) = vista.fetch_next(None).await?;
    let (_p2, tok2) = vista.fetch_next(tok1).await?;
    assert!(tok2.is_none());

    // Passing None again restarts.
    let (p_restart, _) = vista.fetch_next(None).await?;
    let ids: Vec<&String> = p_restart.iter().map(|(id, _)| id).collect();
    assert_eq!(ids, ["a", "b"], "passing None restarts at page 1");
    Ok(())
}

#[tokio::test]
async fn vista_fetch_next_rejects_bad_token_type() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.set_page_size(2)?;

    // SQLite expects CborValue::Integer; pass a Text and expect rejection.
    let bad_token = Some(CborValue::Text("not a page number".into()));
    let result = vista.fetch_next(bad_token).await;
    assert!(result.is_err(), "non-Integer token must be rejected");
    Ok(())
}

#[tokio::test]
async fn vista_fetch_next_without_set_page_size_errors() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    let result = vista.fetch_next(None).await;
    assert!(
        result.is_err(),
        "fetch_next without set_page_size must error"
    );
    Ok(())
}

#[tokio::test]
async fn vista_columns_advertise_orderable_flag() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    for col_name in ["id", "name", "price", "is_deleted"] {
        let col = vista.get_column(col_name).expect("column exists");
        assert!(
            col.has_flag(vantage_vista::flags::ORDERABLE),
            "column '{}' must carry ORDERABLE flag for SQLite",
            col_name
        );
    }
    Ok(())
}

/// A `cart` whose `items` column is a JSON array, surfaced as a contains-many
/// relation. SQLite has no native nesting — the collection round-trips as a
/// JSON string in a TEXT column.
fn cart_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("cart", db)
        .with_id_column("id")
        .with_column_of::<String>("items")
        .with_contained_many(
            "items",
            "items",
            |db| {
                Table::<SqliteDB, EmptyEntity>::new("items", db)
                    .with_column_of::<String>("sku")
                    .with_column_of::<i64>("qty")
            },
            None,
        )
}

fn field(name: &str, value: CborValue) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(name.to_string(), value);
    r
}

#[tokio::test]
async fn contained_json_column_round_trips_on_sqlite() -> TestResult {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE cart (id TEXT PRIMARY KEY, items TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO cart VALUES ('c1', '[{"sku":"a","qty":1},{"sku":"b","qty":2}]')"#)
        .execute(db.pool())
        .await
        .unwrap();

    let vista = db.vista_factory().from_table(cart_table(db.clone()))?;
    assert_eq!(vista.list_contained().len(), 1);

    // The host column reads back as a JSON string; the sub-Vista parses it.
    let cart = vista.get_value("c1").await?.unwrap();
    assert!(matches!(cart.get("items"), Some(CborValue::Text(_))));
    let items = vista.get_ref("items", &cart)?;
    assert_eq!(items.list_values().await?.len(), 2);

    // Add an item — eager writeback serializes to JSON and UPDATEs the column.
    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("c".into()));
    line.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let new_id = items.insert_return_id_value(&line).await?;
    assert_eq!(new_id, "2");

    // Fresh read + re-traverse re-parses the persisted JSON → three items.
    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", &cart2)?;
    let rows = items2.list_values().await?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows["2"].get("sku"), Some(&CborValue::Text("c".into())));

    // Patch one item; confirm it persisted through the JSON column.
    items2
        .patch_value("0", &field("qty", CborValue::Integer(99i64.into())))
        .await?;
    let cart3 = vista.get_value("c1").await?.unwrap();
    let items3 = vista.get_ref("items", &cart3)?;
    assert_eq!(
        items3.get_value("0").await?.unwrap().get("qty"),
        Some(&CborValue::Integer(99i64.into()))
    );
    Ok(())
}

#[tokio::test]
async fn contained_from_yaml_round_trips_on_sqlite() -> TestResult {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE cart_yaml (id TEXT PRIMARY KEY, items TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO cart_yaml VALUES ('c1', '[{"sku":"a","qty":1}]')"#)
        .execute(db.pool())
        .await
        .unwrap();

    let yaml = r#"
name: cart
columns:
  id: { type: string, flags: [id] }
  items: { type: string }
sqlite:
  table: cart_yaml
contained:
  items:
    host_column: items
    kind: contains_many
    columns:
      sku: { type: string }
      qty: { type: int }
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;
    assert_eq!(
        vista.list_contained(),
        vec![(
            "items".to_string(),
            vantage_vista::ContainedKind::ContainsMany
        )]
    );

    let cart = vista.get_value("c1").await?.unwrap();
    let items = vista.get_ref("items", &cart)?;
    assert_eq!(items.list_values().await?.len(), 1);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("b".into()));
    line.insert("qty".to_string(), CborValue::Integer(2i64.into()));
    items.insert_return_id_value(&line).await?;

    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", &cart2)?;
    assert_eq!(items2.list_values().await?.len(), 2);
    assert_eq!(
        items2.get_value("1").await?.unwrap().get("sku"),
        Some(&CborValue::Text("b".into()))
    );
    Ok(())
}
