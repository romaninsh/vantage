//! Vista integration: typed `Table<SqliteDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Uses in-memory SQLite — no external setup required. The cases every SQL
//! backend shares come from `vista_tests!` (`tests/common/vista.rs`).

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::vista::SqliteVistaSpec;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record, entity};
use vantage_vista::{Capability, FilterOp, JoinKey, ReferenceKind, VistaFactory};

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

/// Every test gets its own in-memory database, so `_suffix` names nothing.
async fn product(_suffix: &str) -> (SqliteDB, Table<SqliteDB, EmptyEntity>) {
    let db = setup().await;
    (db.clone(), product_table(db))
}

async fn clients_orders(_suffix: &str) -> (SqliteDB, String, String) {
    let db = setup_clients_orders().await;
    (db, "client".to_string(), "orders".to_string())
}

vista_tests!(SqliteVistaSpec, "sqlite");

#[tokio::test]
async fn vista_lists_typed_sqlite_as_cbor() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), "product");
    assert_eq!(vista.id_column(), Some("id"));

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
async fn vista_pushes_operator_conditions_server_side() -> TestResult {
    // products: a=Alpha/10/0, b=Beta/20/1, c=Gamma/30/0.
    let db = setup().await;

    // capability advertised
    assert!(
        db.vista_factory()
            .from_table(product_table(db.clone()))?
            .capabilities()
            .has(Capability::FilterOps),
        "sqlite advertises operator push-down"
    );

    // `!=` — price != 20 excludes Beta → a, c.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    vista.add_condition("price", FilterOp::Ne, CborValue::Integer(20i64.into()))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a") && rows.contains_key("c") && !rows.contains_key("b"));

    // `>` — price > 15 → Beta, Gamma.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    vista.add_condition("price", FilterOp::Gt, CborValue::Integer(15i64.into()))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("b") && rows.contains_key("c") && !rows.contains_key("a"));

    // `in` — id in {a, c}.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let set = CborValue::Array(vec![
        CborValue::Text("a".into()),
        CborValue::Text("c".into()),
    ]);
    vista.add_condition("id", FilterOp::InSet, set)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a") && rows.contains_key("c") && !rows.contains_key("b"));

    // `not in` — id not in {b}.
    let mut vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let set = CborValue::Array(vec![CborValue::Text("b".into())]);
    vista.add_condition("id", FilterOp::NotInSet, set)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a") && rows.contains_key("c") && !rows.contains_key("b"));

    Ok(())
}

/// The YAML flags with a typed counterpart reach the vista column through
/// the typed table.
#[tokio::test]
async fn vista_yaml_column_flags_reach_the_vista() -> TestResult {
    use vantage_vista::ColumnFlag;

    let db = setup().await;
    let yaml = r#"
name: product
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [searchable, mandatory] }
  is_deleted: { type: bool, flags: [label] }
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;

    let name = vista.column("name").expect("name");
    assert!(name.has_flag(ColumnFlag::Searchable), "{name:?}");
    assert!(name.has_flag(ColumnFlag::Mandatory), "{name:?}");
    let is_deleted = vista.column("is_deleted").expect("is_deleted");
    assert!(
        is_deleted.has_flag(ColumnFlag::from("label")),
        "{is_deleted:?}"
    );
    Ok(())
}

/// A `lazy:` column runs its Rhai script on each returned record (`row` in
/// scope), in declaration order — the second script reads the first's value.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_lazy_columns_compute_from_row() -> TestResult {
    let db = setup().await;

    let yaml = r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
  price:
    type: int
  price_label:
    lazy: '`${row.name}: ${row.price}`'
  shouted:
    lazy: 'row.price_label.to_upper()'
sqlite:
  table: product
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;
    let rows = vista.list_values().await?;
    assert_eq!(
        rows["a"].get("price_label"),
        Some(&CborValue::Text("Alpha: 10".into()))
    );
    assert_eq!(
        rows["a"].get("shouted"),
        Some(&CborValue::Text("ALPHA: 10".into()))
    );

    // Computed by the Vista, so every read path fills it and writes drop it.
    assert!(vista.column("shouted").unwrap().is_computed());
    let window = vista.fetch_window(2, 1).await?;
    assert_eq!(
        window.rows[0].1.get("price_label"),
        Some(&CborValue::Text("Gamma: 30".into()))
    );
    let record: Record<CborValue> = [
        ("name", CborValue::Text("Delta".into())),
        ("price", CborValue::Integer(40i64.into())),
        ("price_label", CborValue::Text("stale".into())),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    vista.insert_value("d", &record).await?;
    let fetched = vista.get_value("d").await?.expect("inserted");
    assert_eq!(
        fetched.get("price_label"),
        Some(&CborValue::Text("Delta: 40".into()))
    );
    Ok(())
}

#[tokio::test]
async fn vista_delete_of_a_missing_row_succeeds() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;
    vista.delete("nope").await?;
    vista.delete("a").await?;
    assert!(vista.get_value("a").await?.is_none());
    vista.delete("a").await?;
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
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::Window,
        Capability::Traverse,
    ] {
        assert!(caps.has(capability), "sqlite advertises {capability}");
    }
    for capability in [
        Capability::Subscribe,
        Capability::Cursor,
        Capability::Import,
        Capability::NativeConditions,
    ] {
        assert!(
            !caps.has(capability),
            "sqlite doesn't advertise {capability}"
        );
    }
    Ok(())
}

pub(super) async fn setup_clients_orders() -> SqliteDB {
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

pub(super) fn orders_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("orders", db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
        .with_column_of::<i64>("total")
}

pub(super) fn clients_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
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

#[tokio::test]
async fn vista_list_references_surfaces_cardinality() -> TestResult {
    let db = setup_clients_orders().await;
    let clients = db.vista_factory().from_table(clients_table(db))?;

    let refs = clients.references();
    assert_eq!(refs.len(), 1);
    let orders = &refs["orders"];
    assert_eq!(orders.kind, ReferenceKind::HasMany);
    assert_eq!(orders.keys, [JoinKey::new("id", "client_id")]);
    Ok(())
}

/// A `base:`-derived vista keeps the `references:` it declares itself, next
/// to whatever it inherits from the base (here: nothing).
#[tokio::test]
async fn vista_yaml_base_derived_declares_own_reference() -> TestResult {
    use indexmap::IndexMap;
    use std::sync::Arc;
    use vantage_sql::sqlite::vista::SqliteVistaSpec;
    use vantage_vista::SpecResolver;

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

#[tokio::test]
async fn vista_set_order_ascending_descending_and_clear() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    // capability is advertised
    assert!(vista.capabilities().has(Capability::Order));

    // sort ascending by price → a (10), b (20), c (30)
    vista.set_order(Some(("price", SortDirection::Ascending)))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a", "b", "c"]);

    // replace-semantics: switch to descending name → c (Gamma), b (Beta), a (Alpha)
    vista.set_order(Some(("name", SortDirection::Descending)))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["c", "b", "a"]);

    // clear → back to insertion order
    vista.set_order(None)?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a", "b", "c"]);
    Ok(())
}

#[tokio::test]
async fn vista_set_order_rejects_unknown_column() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    let err = vista
        .set_order(Some(("not_a_column", SortDirection::Ascending)))
        .unwrap_err();
    assert!(!err.is_unsupported(), "an unknown column is a normal error");
    assert!(
        vista.query().order.is_none(),
        "a refused order isn't recorded"
    );
    Ok(())
}

#[tokio::test]
async fn vista_set_search_filters_results_with_replace_semantics() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert!(vista.capabilities().has(Capability::Search));

    // search "alpha" → only row a (name = "Alpha")
    vista.set_search(Some("alpha"))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(ids, ["a"], "search 'alpha' must match only row a");

    // replace-semantics: search "amma" → only row c (name = "Gamma")
    vista.set_search(Some("amma"))?;
    let rows = vista.list_values().await?;
    let ids: Vec<&String> = rows.keys().collect();
    assert_eq!(
        ids,
        ["c"],
        "search 'amma' must match only row c after replace"
    );

    // clear → all rows back
    vista.set_search(None)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3, "clearing the search must restore all rows");
    Ok(())
}

/// Only `name` is flagged `searchable`, so a needle found only in `body`
/// matches nothing.
#[tokio::test]
async fn sqlite_search_reads_only_searchable_columns() -> TestResult {
    let db = SqliteDB::connect("sqlite::memory:").await?;
    sqlx::query("CREATE TABLE note (id TEXT PRIMARY KEY, name TEXT NOT NULL, body TEXT NOT NULL)")
        .execute(db.pool())
        .await?;
    sqlx::query("INSERT INTO note VALUES ('a', 'Alpha', 'hoverboard'), ('b', 'Beta', 'plutonium')")
        .execute(db.pool())
        .await?;
    let yaml = r#"
name: note
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [searchable] }
  body: { type: string }
"#;
    let mut vista = db.vista_factory().from_yaml(yaml)?;

    vista.set_search(Some("hover"))?;
    assert!(vista.list_values().await?.is_empty(), "body isn't searched");

    vista.set_search(Some("bet"))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["b"]);
    Ok(())
}

#[tokio::test]
async fn vista_clearing_search_without_prior_search_is_noop() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    // No prior search — clear should silently succeed.
    vista.set_search(None)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    Ok(())
}

fn window_ids(page: &vantage_vista::Page) -> Vec<&str> {
    page.rows.iter().map(|(id, _)| id.as_str()).collect()
}

#[tokio::test]
async fn vista_fetch_window_offset_pagination() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    assert!(vista.capabilities().has(Capability::Window));
    vista.set_order(Some(("id", vantage_vista::SortDirection::Ascending)))?;

    let first = vista.fetch_window(0, 2).await?;
    assert_eq!(window_ids(&first), ["a", "b"], "the first two rows");
    assert_eq!(first.total, None, "a window doesn't count the set");

    let second = vista.fetch_window(2, 2).await?;
    assert_eq!(window_ids(&second), ["c"], "the third row");

    let past_the_end = vista.fetch_window(4, 2).await?;
    assert!(past_the_end.rows.is_empty(), "nothing past the end");
    Ok(())
}

/// SQL pages by offset, so it has no cursor: `fetch_cursor` refuses.
#[tokio::test]
async fn vista_fetch_cursor_is_unsupported() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;

    assert!(!vista.capabilities().has(Capability::Cursor));
    let err = vista.fetch_cursor(None, 2).await.unwrap_err();
    assert!(err.is_unsupported());
    Ok(())
}

#[tokio::test]
async fn vista_fetch_window_honors_search_and_order() -> TestResult {
    use vantage_vista::SortDirection;

    let db = setup().await;
    let table = product_table(db.clone());
    let mut vista = db.vista_factory().from_table(table)?;

    vista.set_order(Some(("name", SortDirection::Descending)))?;
    vista.set_search(Some("a"))?; // matches Alpha, Beta, Gamma (all contain 'a' case-insensitively)

    let page = vista.fetch_window(0, 10).await?;
    let names: Vec<String> = page
        .rows
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

/// A clone of a narrowed vista carries the narrowing; narrowing the clone
/// leaves the original alone.
#[tokio::test]
async fn vista_clone_owns_its_narrowing() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let cheap =
        vista
            .clone()
            .with_condition("price", FilterOp::Lt, CborValue::Integer(25i64.into()))?;
    let alpha = cheap.clone().with_search("alpha")?;

    assert_eq!(vista.count().await?, 3);
    assert_eq!(cheap.count().await?, 2);
    assert_eq!(alpha.count().await?, 1);
    assert_eq!(alpha.query().search.as_deref(), Some("alpha"));
    assert!(cheap.query().search.is_none());
    Ok(())
}

#[tokio::test]
async fn vista_preview_renders_the_select() -> TestResult {
    let db = setup().await;
    let vista = db
        .vista_factory()
        .from_table(product_table(db.clone()))?
        .with_condition("price", FilterOp::Gt, CborValue::Integer(15i64.into()))?;
    let preview = vista.preview();
    assert_eq!(vista.driver_name(), "sqlite");
    // Every condition is pushed into the SELECT, so nothing is left to note.
    assert_eq!(preview.note, None);
    let sql = preview.query.expect("a SELECT");
    assert!(sql.contains("product") && sql.contains("15"), "{sql}");
    Ok(())
}

#[tokio::test]
async fn vista_upsert_creates_then_replaces() -> TestResult {
    let db = setup().await;
    let vista = db.vista_factory().from_table(product_table(db.clone()))?;
    let row = |name: &str| -> Record<CborValue> {
        [
            ("name", CborValue::Text(name.into())),
            ("price", CborValue::Integer(5i64.into())),
            ("is_deleted", CborValue::Bool(false)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    };
    vista.upsert_value("z", &row("Zeta")).await?;
    vista.upsert_value("z", &row("Zeta 2")).await?;
    let stored = vista.get_value("z").await?.expect("upserted");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Zeta 2".into())));
    assert_eq!(vista.count().await?, 4);
    Ok(())
}

#[tokio::test]
async fn vista_columns_advertise_orderable_flag() -> TestResult {
    let db = setup().await;
    let table = product_table(db.clone());
    let vista = db.vista_factory().from_table(table)?;

    for col_name in ["id", "name", "price", "is_deleted"] {
        let col = vista.column(col_name).expect("column exists");
        assert!(
            col.has_flag(vantage_vista::ColumnFlag::Orderable),
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
    assert_eq!(
        vista.references()["items"].kind,
        ReferenceKind::ContainsMany
    );

    // Without a parent row, the bare target: the schema, no records, no writes.
    let bare = vista.get_ref("items", None)?;
    assert!(bare.column("sku").is_some());
    assert!(bare.list_values().await?.is_empty());
    assert!(!bare.capabilities().has(Capability::Insert));

    // The host column reads back as a JSON string; the sub-Vista parses it.
    let cart = vista.get_value("c1").await?.unwrap();
    assert!(matches!(cart.get("items"), Some(CborValue::Text(_))));
    let items = vista.get_ref("items", Some(&cart))?;
    assert_eq!(items.list_values().await?.len(), 2);

    // Add an item — eager writeback serializes to JSON and UPDATEs the column.
    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("c".into()));
    line.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let new_id = items.insert_return_id_value(&line).await?;
    assert_eq!(new_id, "2");

    // Fresh read + re-traverse re-parses the persisted JSON → three items.
    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", Some(&cart2))?;
    let rows = items2.list_values().await?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows["2"].get("sku"), Some(&CborValue::Text("c".into())));

    // Patch one item; confirm it persisted through the JSON column.
    items2
        .patch_value("0", &field("qty", CborValue::Integer(99i64.into())))
        .await?;
    let cart3 = vista.get_value("c1").await?.unwrap();
    let items3 = vista.get_ref("items", Some(&cart3))?;
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
    let items_ref = &vista.references()["items"];
    assert_eq!(items_ref.kind, ReferenceKind::ContainsMany);
    assert_eq!(items_ref.host_column.as_deref(), Some("items"));

    let cart = vista.get_value("c1").await?.unwrap();
    let items = vista.get_ref("items", Some(&cart))?;
    assert_eq!(items.list_values().await?.len(), 1);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("b".into()));
    line.insert("qty".to_string(), CborValue::Integer(2i64.into()));
    items.insert_return_id_value(&line).await?;

    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", Some(&cart2))?;
    assert_eq!(items2.list_values().await?.len(), 2);
    assert_eq!(
        items2.get_value("1").await?.unwrap().get("sku"),
        Some(&CborValue::Text("b".into()))
    );
    Ok(())
}
