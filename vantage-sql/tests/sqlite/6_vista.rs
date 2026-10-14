//! Vista integration: typed `Table<SqliteDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Uses in-memory SQLite — no external setup required. The cases every SQL
//! backend shares come from `vista_tests!` (`tests/common/vista.rs`); the
//! SQLite-only cases live in the child modules, by topic.

#![cfg(feature = "vista")]

use std::error::Error;

use vantage_dataset::prelude::*;
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::vista::SqliteVistaSpec;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::VistaFactory;

#[path = "6_vista/contained.rs"]
mod contained;
#[path = "6_vista/query.rs"]
mod query;
#[path = "6_vista/refs.rs"]
mod refs;
#[path = "6_vista/typed.rs"]
mod typed;
#[path = "6_vista/yaml_columns.rs"]
mod yaml_columns;

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
