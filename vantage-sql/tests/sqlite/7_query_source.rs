//! Test 7: tables sourced from an arbitrary SELECT query.
//!
//! Covers `Table::from_select` (a raw query as a read-only source) and
//! `Table::derive_from` (deriving a table from another, inheriting columns and
//! relations). Uses an in-memory database, so no ingress setup is required.

use vantage_expressions::{ExprDataSource, Selectable};
#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::statements::SqliteSelect;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, entity};

#[cfg(feature = "vista")]
use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
#[cfg(feature = "vista")]
use vantage_dataset::WritableValueSet;
#[cfg(feature = "vista")]
use vantage_types::Record;
#[cfg(feature = "vista")]
use vantage_vista::Capability;

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE client (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            is_paying_client BOOLEAN NOT NULL DEFAULT 0
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query("CREATE TABLE client_order (id TEXT PRIMARY KEY, client_id TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();

    let insert = sqlite_expr!(
        "INSERT INTO client VALUES ({}, {}, {}), ({}, {}, {}), ({}, {}, {})",
        "1",
        "Alice",
        true,
        "2",
        "Bob",
        false,
        "3",
        "Carol",
        true
    );
    db.execute(&insert).await.unwrap();
    db
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Client {
    name: String,
    is_paying_client: bool,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ClientOrder {
    client_id: String,
}

fn client_table(db: SqliteDB) -> Table<SqliteDB, Client> {
    Table::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client")
        .with_many("orders", "client_id", order_table)
}

fn order_table(db: SqliteDB) -> Table<SqliteDB, ClientOrder> {
    Table::new("client_order", db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
}

/// A hand-built query used directly as a read-only table source.
#[tokio::test]
async fn from_select_renders_subquery_source() {
    let db = setup().await;

    let query = SqliteSelect::new()
        .with_source("client")
        .with_field("id")
        .with_field("name");

    let derived: Table<SqliteDB, EmptyEntity> = Table::from_select(db, "paying", query)
        .with_id_column("id")
        .with_column_of::<String>("name");

    assert_eq!(
        derived.select().preview(),
        "SELECT \"id\", \"name\" FROM (SELECT \"id\", \"name\" FROM \"client\") AS \"paying\""
    );
}

/// Derive a table from another: filter via the modifier, inherit columns.
#[tokio::test]
async fn derive_from_inherits_columns_and_filters() {
    let db = setup().await;
    let clients = client_table(db);

    let debtors: Table<SqliteDB, EmptyEntity> = Table::derive_from(
        &clients,
        "debtors",
        |sel| sel.with_condition(sqlite_expr!("{} = {}", (clients["is_paying_client"]), true)),
        &["id", "name"],
        &[],
    );

    assert_eq!(
        debtors.select().preview(),
        "SELECT \"id\", \"name\" FROM (SELECT \"id\", \"name\", \"is_paying_client\" \
         FROM \"client\" WHERE is_paying_client = 1) AS \"debtors\""
    );

    // End-to-end: only the two paying clients come back.
    let rows = debtors.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
}

/// An inherited relation keeps working: it traverses against the derived set.
#[tokio::test]
async fn derive_from_inherits_relation() {
    let db = setup().await;
    let clients = client_table(db);

    let debtors: Table<SqliteDB, EmptyEntity> = Table::derive_from(
        &clients,
        "debtors",
        |sel| sel.with_condition(sqlite_expr!("{} = {}", (clients["is_paying_client"]), true)),
        &["id", "name"],
        &["orders"],
    );

    assert!(debtors.references().contains(&"orders".to_string()));

    let orders = debtors.get_ref_as::<ClientOrder>("orders").unwrap();
    assert_eq!(
        orders.select().preview(),
        "SELECT \"id\", \"client_id\" FROM \"client_order\" WHERE client_id IN \
         (SELECT \"id\" FROM (SELECT \"id\", \"name\", \"is_paying_client\" \
         FROM \"client\" WHERE is_paying_client = 1) AS \"debtors\")"
    );
}

/// A reference whose target is query-sourced: the target vista is built by
/// `from_table`, which must see the query source and make it read-only, as
/// `build_from_spec` does for a `rhai:` or `base:` spec (VAN-461).
#[cfg(feature = "vista")]
#[tokio::test]
async fn a_query_source_reached_by_reference_refuses_insert() {
    let db = setup().await;
    db.execute(&sqlite_expr!(
        "INSERT INTO client_order VALUES ({}, {})",
        "o1",
        "1"
    ))
    .await
    .unwrap();
    // Named after the real table, so a write that got through would land.
    let clients = client_table(db.clone()).with_many("order_view", "client_id", |db| {
        let query = SqliteSelect::new()
            .with_source("client_order")
            .with_field("id")
            .with_field("client_id");
        Table::<SqliteDB, EmptyEntity>::from_select(db, "client_order", query)
            .with_id_column("id")
            .with_column_of::<String>("client_id")
    });
    let vista = db.vista_factory().from_table(clients).unwrap();
    let alice = vista.get_value("1").await.unwrap().expect("client 1");

    let target = vista.get_ref("order_view", Some(&alice)).unwrap();
    assert!(!target.capabilities().has(Capability::Insert));
    let client = |id: &str| -> Record<CborValue> {
        [("client_id".to_string(), CborValue::Text(id.into()))]
            .into_iter()
            .collect()
    };
    let err = target.insert_value("o9", &client("1")).await.unwrap_err();
    assert!(err.is_forbidden() || err.is_unsupported(), "{err:?}");
    let err = target.patch_value("o1", &client("2")).await.unwrap_err();
    assert!(err.is_forbidden() || err.is_unsupported(), "{err:?}");

    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, client_id FROM client_order")
        .fetch_all(db.pool())
        .await
        .unwrap();
    assert_eq!(rows, vec![("o1".to_string(), "1".to_string())]);
}
