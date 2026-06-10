//! Vista integration: typed `Table<PostgresDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running PostgreSQL on `postgres://vantage:vantage@localhost:5433/vantage`.
//! Each test uses a uniquely-suffixed table to avoid collisions.

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::postgres::PostgresDB;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::VistaFactory;

type TestResult = std::result::Result<(), Box<dyn Error>>;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

async fn setup(suffix: &str) -> (PostgresDB, String) {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let table_name = format!("vista_product_{}", suffix);

    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table_name))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            is_deleted BOOLEAN NOT NULL DEFAULT false
        )",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES \
         ('a', 'Alpha', 10, false), \
         ('b', 'Beta', 20, true), \
         ('c', 'Gamma', 30, false)",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    (db, table_name)
}

fn product_table(db: PostgresDB, name: &str) -> Table<PostgresDB, EmptyEntity> {
    Table::<PostgresDB, EmptyEntity>::new(name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

#[tokio::test]
async fn vista_lists_typed_postgres_as_cbor() -> TestResult {
    let (db, name) = setup("list").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), name);
    assert_eq!(vista.get_id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);

    let alpha = rows.get("a").expect("row a");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    Ok(())
}

#[tokio::test]
async fn vista_get_value_by_id() -> TestResult {
    let (db, name) = setup("get").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    let row = vista.get_value("b").await?.expect("row b exists");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Beta".to_string())));

    let missing = vista.get_value("nope").await?;
    assert!(missing.is_none());
    Ok(())
}

#[tokio::test]
async fn vista_count_with_eq_condition() -> TestResult {
    let (db, name) = setup("count").await;
    let table = product_table(db.clone(), &name);
    let mut vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.get_count().await?, 3);

    vista.add_condition_eq("is_deleted", CborValue::Bool(false))?;
    assert_eq!(vista.get_count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a"));
    assert!(rows.contains_key("c"));
    Ok(())
}

#[tokio::test]
async fn vista_yaml_loads_table_and_columns() -> TestResult {
    let (db, name) = setup("yaml").await;

    let yaml = format!(
        r#"
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
postgres:
  table: {name}
"#
    );

    let vista = db.vista_factory().from_yaml(&yaml)?;

    assert_eq!(vista.name(), "product_view");
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    Ok(())
}

#[tokio::test]
async fn vista_writes_round_trip_via_cbor() -> TestResult {
    let (db, name) = setup("write").await;
    let table = product_table(db.clone(), &name);
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

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, name) = setup("caps").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(caps.can_insert);
    assert!(caps.can_update);
    assert!(caps.can_delete);
    assert!(!caps.can_subscribe);
    Ok(())
}

fn field(name: &str, value: CborValue) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(name.to_string(), value);
    r
}

/// A `cart` whose `items` column is a JSON array (TEXT), surfaced as a
/// contains-many relation — same JSON-blob round-trip as SQLite.
fn cart_table(db: PostgresDB, table: &str) -> Table<PostgresDB, EmptyEntity> {
    Table::<PostgresDB, EmptyEntity>::new(table, db)
        .with_id_column("id")
        .with_column_of::<String>("items")
        .with_contained_many(
            "items",
            "items",
            |db| {
                Table::new("items", db)
                    .with_column_of::<String>("sku")
                    .with_column_of::<i64>("qty")
            },
            None,
        )
}

#[tokio::test]
async fn contained_json_column_round_trips_on_postgres() -> TestResult {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let t = "vista_cart_contained";
    sqlx::query(&format!("DROP TABLE IF EXISTS \"{t}\""))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE \"{t}\" (id TEXT PRIMARY KEY, items TEXT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        r#"INSERT INTO "{t}" VALUES ('c1', '[{{"sku":"a","qty":1}},{{"sku":"b","qty":2}}]')"#
    ))
    .execute(db.pool())
    .await
    .unwrap();

    let vista = db.vista_factory().from_table(cart_table(db.clone(), t))?;
    assert_eq!(vista.list_contained().len(), 1);

    let cart = vista.get_value("c1").await?.unwrap();
    assert!(matches!(cart.get("items"), Some(CborValue::Text(_))));
    let items = vista.get_ref("items", &cart)?;
    assert_eq!(items.list_values().await?.len(), 2);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("c".into()));
    line.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let new_id = items.insert_return_id_value(&line).await?;
    assert_eq!(new_id, "2");

    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", &cart2)?;
    assert_eq!(items2.list_values().await?.len(), 3);

    items2
        .patch_value("0", &field("qty", CborValue::Integer(99i64.into())))
        .await?;
    let cart3 = vista.get_value("c1").await?.unwrap();
    let items3 = vista.get_ref("items", &cart3)?;
    assert_eq!(
        items3.get_value("0").await?.unwrap().get("qty"),
        Some(&CborValue::Integer(99i64.into()))
    );

    sqlx::query(&format!("DROP TABLE \"{t}\""))
        .execute(db.pool())
        .await
        .ok();
    Ok(())
}

/// A vista whose source is a Rhai-built SELECT (not a physical table). The
/// script filters `price > 15`, so only two products surface, and the vista is
/// read-only.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_rhai_source_is_read_only_and_filters() -> TestResult {
    let (db, name) = setup("rhai").await;

    let yaml = format!(
        r#"
name: expensive_products
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
postgres:
  rhai: |
    select().from("{name}").field("id").field("name").where(expr("price > 15"))
"#
    );

    let vista = db.vista_factory().from_yaml(&yaml)?;
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

/// Create suffixed `client` and `orders` tables and return their names.
async fn setup_clients_orders(suffix: &str) -> (PostgresDB, String, String) {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let client_t = format!("vista_client_{suffix}");
    let orders_t = format!("vista_orders_{suffix}");

    for t in [&client_t, &orders_t] {
        sqlx::query(&format!("DROP TABLE IF EXISTS \"{t}\""))
            .execute(db.pool())
            .await
            .unwrap();
    }
    sqlx::query(&format!(
        "CREATE TABLE \"{client_t}\" (id TEXT PRIMARY KEY, name TEXT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE \"{orders_t}\" (id TEXT PRIMARY KEY, client_id TEXT NOT NULL, total BIGINT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO \"{client_t}\" VALUES ('alice','Alice'),('bob','Bob')"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO \"{orders_t}\" VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    (db, client_t, orders_t)
}

/// YAML-declared `references:` lower to `with_one`/`with_many`, with the target
/// table resolved by name through a spec resolver attached to the factory.
#[tokio::test]
async fn vista_yaml_references_resolve_via_resolver() -> TestResult {
    use indexmap::IndexMap;
    use std::sync::Arc;
    use vantage_sql::postgres::vista::{PostgresSpecResolver, PostgresVistaSpec};

    let (db, client_t, orders_t) = setup_clients_orders("refs").await;

    let client_yaml = format!(
        r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
postgres:
  table: {client_t}
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
"#
    );
    let orders_yaml = format!(
        r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
postgres:
  table: {orders_t}
"#
    );

    let client_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&client_yaml)?;
    let orders_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&orders_yaml)?;

    let map: IndexMap<String, PostgresVistaSpec> =
        [("orders".to_string(), orders_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: PostgresSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

    let factory = db.vista_factory().with_resolver(resolver);
    let mut clients = factory.build_from_spec(client_spec)?;

    let refs = clients.list_references();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].0, "orders");
    assert_eq!(refs[0].1, vantage_vista::ReferenceKind::HasMany);

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

/// A derived vista: `base: client` resolves eagerly through the resolver, the
/// base table's `select()` is seeded into the Rhai engine as `base` (transform
/// mode), and the listed columns are inherited. Query source → read-only.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_base_inherits_columns_and_transforms() -> TestResult {
    use indexmap::IndexMap;
    use std::sync::Arc;
    use vantage_sql::postgres::vista::{PostgresSpecResolver, PostgresVistaSpec};

    let (db, client_t, _orders_t) = setup_clients_orders("base").await;

    let client_yaml = format!(
        r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
postgres:
  table: {client_t}
"#
    );
    let vip_yaml = r#"
name: vip_clients
columns: {}
postgres:
  base: client
  inherit:
    columns: [id, name]
  rhai: |
    base.where(expr("name = 'Alice'"))
"#;

    let client_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&client_yaml)?;
    let vip_spec: PostgresVistaSpec = serde_yaml_ng::from_str(vip_yaml)?;

    let map: IndexMap<String, PostgresVistaSpec> =
        [("client".to_string(), client_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: PostgresSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

    let vista = db
        .vista_factory()
        .with_resolver(resolver)
        .build_from_spec(vip_spec)?;

    let caps = vista.capabilities();
    assert!(
        !caps.can_insert,
        "derived (query-sourced) vista is read-only"
    );
    assert!(!caps.can_update);
    assert!(!caps.can_delete);

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
    use vantage_sql::postgres::vista::{PostgresSpecResolver, PostgresVistaSpec};

    let (db, _client_t, orders_t) = setup_clients_orders("agg").await;

    let orders_yaml = format!(
        r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
postgres:
  table: {orders_t}
"#
    );
    let totals_yaml = r#"
name: client_totals
id_column: client_id
columns:
  total_due: { type: int }
postgres:
  base: orders
  inherit:
    columns: [client_id]
  rhai: |
    base.clear_fields().field("client_id").expression(expr("SUM(total) AS total_due")).group_by(expr("client_id"))
"#;

    let orders_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&orders_yaml)?;
    let totals_spec: PostgresVistaSpec = serde_yaml_ng::from_str(totals_yaml)?;

    let map: IndexMap<String, PostgresVistaSpec> =
        [("orders".to_string(), orders_spec)].into_iter().collect();
    let map = Arc::new(map);
    let resolver: PostgresSpecResolver = Arc::new(move |name: &str| map.get(name).cloned());

    let vista = db
        .vista_factory()
        .with_resolver(resolver)
        .build_from_spec(totals_spec)?;

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2, "one row per client");
    // Postgres `SUM(bigint)` returns `numeric`, which surfaces as a CBOR decimal
    // rather than the integer SQLite produces — normalise before comparing.
    assert_eq!(
        numeric(rows.get("alice").unwrap().get("total_due")),
        Some(30)
    );
    assert_eq!(numeric(rows.get("bob").unwrap().get("total_due")), Some(30));
    Ok(())
}

/// Extract an integer from a CBOR scalar regardless of whether the backend
/// returned an integer or a decimal (numeric → `Tag(_, Text("30"))`).
#[cfg(feature = "rhai")]
fn numeric(v: Option<&CborValue>) -> Option<i64> {
    match v? {
        CborValue::Integer(i) => i64::try_from(*i).ok(),
        CborValue::Text(s) => s.parse().ok(),
        CborValue::Tag(_, inner) => match inner.as_ref() {
            CborValue::Text(s) => s.parse().ok(),
            _ => None,
        },
        _ => None,
    }
}
