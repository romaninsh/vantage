//! `vantage_vista::contract` over in-memory SQLite.

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use vantage_sql::sqlite::SqliteDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::contract;

/// Clients `alice` and `bob`; orders `o1`, `o2` (alice) and `o3` (bob).
async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    for sql in [
        "CREATE TABLE client (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE orders (id TEXT PRIMARY KEY, client_id TEXT, total INTEGER)",
        "INSERT INTO client VALUES ('alice','Alice'),('bob','Bob')",
        "INSERT INTO orders VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)",
    ] {
        sqlx::query(sql).execute(db.pool()).await.unwrap();
    }
    db
}

fn orders(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("orders", db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
        .with_column_of::<i64>("total")
}

fn clients(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    let target = db.clone();
    Table::<SqliteDB, EmptyEntity>::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("orders", "client_id", move |_| orders(target.clone()))
}

#[tokio::test]
async fn sqlite_contract_advertised() {
    let db = setup().await;
    let factory = db.vista_factory();
    contract::check_advertised(&factory.from_table(clients(db.clone())).unwrap()).await;
    contract::check_advertised_writes(&factory.from_table(orders(db.clone())).unwrap()).await;
}

#[tokio::test]
async fn sqlite_contract_clone() {
    let db = setup().await;
    let vista = db.vista_factory().from_table(orders(db.clone())).unwrap();
    contract::check_clone(&vista, "client_id", CborValue::Text("alice".into())).await;
}
