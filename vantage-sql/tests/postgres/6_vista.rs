//! Vista integration: typed `Table<PostgresDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running PostgreSQL on `postgres://vantage:vantage@localhost:5433/vantage`.
//! Each test uses a uniquely-suffixed table to avoid collisions. The cases
//! every SQL backend shares come from `vista_tests!` (`tests/common/vista.rs`).

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::postgres::PostgresDB;
use vantage_sql::postgres::vista::PostgresVistaSpec;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, ReferenceKind, VistaFactory};

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

async fn product(suffix: &str) -> (PostgresDB, Table<PostgresDB, EmptyEntity>) {
    let (db, name) = setup(suffix).await;
    (db.clone(), product_table(db, &name))
}

vista_tests!(PostgresVistaSpec, "postgres");

#[tokio::test]
async fn vista_lists_typed_postgres_as_cbor() -> TestResult {
    let (db, name) = setup("list").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), name);
    assert_eq!(vista.id_column(), Some("id"));

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
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, name) = setup("caps").await;
    let table = product_table(db.clone(), &name);
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
        assert!(caps.has(capability), "postgres advertises {capability}");
    }
    assert!(!caps.has(Capability::Cursor));
    // Being writable says nothing about whether anyone installed the
    // `{table}_changed` trigger, so watchability is NOT inferred from it.
    assert!(!caps.has(Capability::Subscribe));
    Ok(())
}

#[tokio::test]
async fn vista_is_watchable_only_when_notify_is_declared() -> TestResult {
    let (db, name) = setup("caps_notify").await;

    // Opting in is what makes the LISTEN/NOTIFY feed advertisable — the
    // application is the only party that knows the trigger exists.
    for notify in [true, false] {
        let vista = db
            .vista_factory()
            .with_notify(notify)
            .from_table(product_table(db.clone(), &name))?;
        assert_eq!(vista.capabilities().has(Capability::Subscribe), notify);
    }
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
    assert_eq!(
        vista.references()["items"].kind,
        ReferenceKind::ContainsMany
    );

    let cart = vista.get_value("c1").await?.unwrap();
    assert!(matches!(cart.get("items"), Some(CborValue::Text(_))));
    let items = vista.get_ref("items", Some(&cart))?;
    assert_eq!(items.list_values().await?.len(), 2);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("c".into()));
    line.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let new_id = items.insert_return_id_value(&line).await?;
    assert_eq!(new_id, "2");

    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", Some(&cart2))?;
    assert_eq!(items2.list_values().await?.len(), 3);

    items2
        .patch_value("0", &field("qty", CborValue::Integer(99i64.into())))
        .await?;
    let cart3 = vista.get_value("c1").await?.unwrap();
    let items3 = vista.get_ref("items", Some(&cart3))?;
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

/// Create suffixed `client` and `orders` tables and return their names.
async fn clients_orders(suffix: &str) -> (PostgresDB, String, String) {
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

/// The `client` spec (with a `has_many` to `orders`) plus a resolver that can
/// find the `orders` target by name.
#[allow(clippy::type_complexity)]
fn clients_orders_specs(
    client_t: &str,
    orders_t: &str,
) -> std::result::Result<
    (
        PostgresVistaSpec,
        std::sync::Arc<dyn vantage_vista::SpecResolver<PostgresVistaSpec>>,
    ),
    Box<dyn Error>,
> {
    let client_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&format!(
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
    ))?;
    let orders_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&format!(
        r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
postgres:
  table: {orders_t}
"#
    ))?;
    Ok((client_spec, spec_resolver(vec![("orders", orders_spec)])))
}

/// Traversing a reference builds a fresh factory internally, which must carry
/// the parent's `with_notify` declaration across — otherwise every relation
/// target on a triggered database comes back silently unwatchable.
#[tokio::test]
async fn notify_opt_in_survives_reference_traversal() -> TestResult {
    let (db, client_t, orders_t) = clients_orders("refs_notify").await;
    let (client_spec, resolver) = clients_orders_specs(&client_t, &orders_t)?;

    for notify in [true, false] {
        let clients = db
            .vista_factory()
            .with_resolver(resolver.clone())
            .with_notify(notify)
            .build_from_spec(client_spec.clone())?;
        assert_eq!(clients.capabilities().has(Capability::Subscribe), notify);

        let alice = alice_row(&clients).await?;
        let orders = clients.get_ref("orders", Some(&alice))?;
        assert_eq!(
            orders.capabilities().has(Capability::Subscribe),
            notify,
            "a target narrowed to a row should inherit the parent's notify opt-in"
        );

        let target = clients.get_ref("orders", None)?;
        assert_eq!(
            target.capabilities().has(Capability::Subscribe),
            notify,
            "the bare target should inherit the parent's notify opt-in"
        );
    }
    Ok(())
}

#[tokio::test]
async fn vista_set_order_ascending_descending_and_clear() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, name) = setup("order").await;
    let table = product_table(db.clone(), &name);
    let mut vista = db.vista_factory().from_table(table)?;

    // A table-sourced Postgres Vista advertises ordering and flags every column
    // ORDERABLE, so `set_order` works without going through YAML.
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

    // clear → every row still present (order unconstrained)
    vista.set_order(None)?;
    let rows = vista.list_values().await?;
    let mut ids: Vec<String> = rows.keys().cloned().collect();
    ids.sort();
    assert_eq!(ids, ["a", "b", "c"]);
    Ok(())
}

/// A window scan must visit every row even when the id column is not unique.
///
/// A window keys rows by id, so duplicates collapse and a FULL SQL window
/// can arrive under-length. Only an empty window ends the scan.
#[tokio::test]
async fn vista_window_scan_survives_a_non_unique_id_column() -> TestResult {
    use vantage_vista::SortDirection;

    let (db, name) = setup("dup_ids").await;
    // Page by `name`, which repeats: 'dup' twice, then a distinct third row.
    sqlx::query(&format!("DELETE FROM \"{}\"", name))
        .execute(db.pool())
        .await?;
    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES \
         ('a', 'dup', 10, false), \
         ('b', 'dup', 20, false), \
         ('c', 'tail', 30, false)",
        name
    ))
    .execute(db.pool())
    .await?;

    let table = Table::<PostgresDB, EmptyEntity>::new(&name, db.clone())
        .with_id_column("name")
        .with_column_of::<String>("id")
        .with_column_of::<i64>("price");
    let mut vista = db.vista_factory().from_table(table)?;
    vista.set_order(Some(("id", SortDirection::Ascending)))?;

    let mut seen: Vec<String> = Vec::new();
    for offset in (0..20).step_by(2) {
        let page = vista.fetch_window(offset, 2).await?;
        if page.rows.is_empty() {
            break;
        }
        seen.extend(page.rows.into_iter().map(|(id, _)| id));
    }
    // 'dup' collapses (the first window returns a single record), but the
    // scan must still reach 'tail'.
    assert!(
        seen.contains(&"tail".to_string()),
        "scan ended early on a deduped window; saw {seen:?}"
    );
    Ok(())
}

/// Paging with no caller-supplied order must still be a stable window:
/// `LIMIT`/`OFFSET` without `ORDER BY` lets Postgres return rows in any
/// order, so pages could repeat one row and skip another.
#[tokio::test]
async fn vista_paging_without_an_order_still_visits_every_row_once() -> TestResult {
    let (db, name) = setup("unordered_pages").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    let mut seen: Vec<String> = Vec::new();
    for offset in [0, 2] {
        let page = vista.fetch_window(offset, 2).await?;
        seen.extend(page.rows.into_iter().map(|(id, _)| id));
    }
    seen.sort();
    assert_eq!(seen, ["a", "b", "c"], "each id must appear exactly once");
    Ok(())
}
