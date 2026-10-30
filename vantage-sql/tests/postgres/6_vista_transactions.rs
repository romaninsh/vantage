//! `vantage_vista::contract::check_transactions` over PostgreSQL.
//!
//! Requires a running PostgreSQL on `postgres://vantage:vantage@localhost:5433/vantage`.
//! Every `fresh` call makes its own suffixed tables.

#![cfg(feature = "vista")]

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use vantage_sql::postgres::PostgresDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::contract::{self, TxFixture, TxTables};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

struct PostgresTx {
    db: PostgresDB,
    next: AtomicUsize,
}

impl PostgresTx {
    async fn new() -> Self {
        Self {
            db: PostgresDB::connect(PG_URL).await.unwrap(),
            next: AtomicUsize::new(0),
        }
    }

    /// Names of two new, empty tables.
    async fn create(&self) -> (String, String) {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let (a, b) = (format!("contract_tx_a_{n}"), format!("contract_tx_b_{n}"));
        for sql in [
            format!("DROP TABLE IF EXISTS \"{a}\""),
            format!("DROP TABLE IF EXISTS \"{b}\""),
            format!("CREATE TABLE \"{a}\" (id TEXT PRIMARY KEY, n BIGINT NOT NULL CHECK (n >= 0))"),
            format!(
                "CREATE TABLE \"{b}\" (id TEXT PRIMARY KEY, a_id TEXT, n BIGINT NOT NULL CHECK (n >= 0))"
            ),
        ] {
            sqlx::query(&sql).execute(self.db.pool()).await.unwrap();
        }
        (a, b)
    }
}

fn tables(db: PostgresDB, (a, b): (String, String)) -> TxTables {
    let factory = db.vista_factory();
    TxTables::new(
        factory.from_table(table_a(db.clone(), &a, &b)).unwrap(),
        factory.from_table(table_b(db, &b)).unwrap(),
    )
}

fn table_a(db: PostgresDB, a: &str, b: &str) -> Table<PostgresDB, EmptyEntity> {
    let target = db.clone();
    let b = b.to_string();
    Table::<PostgresDB, EmptyEntity>::new(a, db)
        .with_id_column("id")
        .with_column_of::<i64>("n")
        .with_many("b", "a_id", move |_| table_b(target.clone(), &b))
}

fn table_b(db: PostgresDB, b: &str) -> Table<PostgresDB, EmptyEntity> {
    Table::<PostgresDB, EmptyEntity>::new(b, db)
        .with_id_column("id")
        .with_column_of::<String>("a_id")
        .with_column_of::<i64>("n")
}

#[async_trait]
impl TxFixture for PostgresTx {
    async fn fresh(&self) -> TxTables {
        let names = self.create().await;
        tables(self.db.clone(), names)
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        let names = self.create().await;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(PG_URL)
            .await
            .unwrap();
        tables(PostgresDB::new(pool), names)
    }

    fn detects_deadlocks(&self) -> bool {
        true
    }

    fn supports_lock(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn postgres_transactions_hold_the_contract() {
    contract::check_transactions(&PostgresTx::new().await).await
}
