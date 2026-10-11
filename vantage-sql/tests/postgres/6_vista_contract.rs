//! `vantage_vista::contract` over PostgreSQL.
//!
//! Requires a running PostgreSQL on `postgres://vantage:vantage@localhost:5433/vantage`.
//! Each test uses its own suffixed tables to avoid collisions.

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use vantage_sql::postgres::PostgresDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::contract;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

/// Clients `alice` and `bob`; orders `o1`, `o2` (alice) and `o3` (bob), in
/// tables named `contract_client_<suffix>` and `contract_orders_<suffix>`.
async fn setup(suffix: &str) -> PostgresDB {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let (client, orders) = names(suffix);
    for sql in [
        format!("DROP TABLE IF EXISTS \"{client}\""),
        format!("DROP TABLE IF EXISTS \"{orders}\""),
        format!("CREATE TABLE \"{client}\" (id TEXT PRIMARY KEY, name TEXT NOT NULL)"),
        format!("CREATE TABLE \"{orders}\" (id TEXT PRIMARY KEY, client_id TEXT, total BIGINT)"),
        format!("INSERT INTO \"{client}\" VALUES ('alice','Alice'),('bob','Bob')"),
        format!(
            "INSERT INTO \"{orders}\" VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)"
        ),
    ] {
        sqlx::query(&sql).execute(db.pool()).await.unwrap();
    }
    db
}

fn names(suffix: &str) -> (String, String) {
    (
        format!("contract_client_{suffix}"),
        format!("contract_orders_{suffix}"),
    )
}

fn orders(db: PostgresDB, suffix: &str) -> Table<PostgresDB, EmptyEntity> {
    Table::<PostgresDB, EmptyEntity>::new(&names(suffix).1, db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
        .with_column_of::<i64>("total")
}

fn clients(db: PostgresDB, suffix: &str) -> Table<PostgresDB, EmptyEntity> {
    let target = db.clone();
    let suffix_owned = suffix.to_string();
    Table::<PostgresDB, EmptyEntity>::new(&names(suffix).0, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("orders", "client_id", move |_| {
            orders(target.clone(), &suffix_owned)
        })
}

#[tokio::test]
async fn postgres_contract_advertised() {
    let db = setup("advertised").await;
    let factory = db.vista_factory();
    let client = factory
        .from_table(clients(db.clone(), "advertised"))
        .unwrap();
    contract::check_advertised(&client).await;
    let orders = factory
        .from_table(orders(db.clone(), "advertised"))
        .unwrap();
    contract::check_advertised_writes(&orders).await;
}

#[tokio::test]
async fn postgres_contract_clone() {
    let db = setup("clone").await;
    let vista = db
        .vista_factory()
        .from_table(orders(db.clone(), "clone"))
        .unwrap();
    contract::check_clone(&vista, "client_id", CborValue::Text("alice".into())).await;
}
