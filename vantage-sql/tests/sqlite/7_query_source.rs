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

use vantage_dataset::ReadableValueSet;

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
