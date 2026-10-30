//! `vantage_vista::contract::check_transactions` and `check_atomic_rhai` over
//! a live SurrealDB 3 (`SURREALDB_URL`, defaults to localhost:8000). Each
//! fixture works in a randomised database of its own; every `fresh` call
//! defines its own numbered tables there.

#![cfg(feature = "vista")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use surreal_client::{SurrealClient, SurrealConnection, WsCborEngine};
use tokio::time::timeout;
use uuid::Uuid;
use vantage_dataset::prelude::*;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::Transaction;
use vantage_vista::contract::{self, TxFixture, TxTables};

fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

async fn connect(database: &str) -> SurrealDB {
    let client = SurrealConnection::dsn(surreal_dsn(database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

/// A client over a bare engine: it can't open a connection of its own, so
/// it has no transactions.
async fn connect_bare(database: &str) -> SurrealDB {
    let connection = SurrealConnection::dsn(surreal_dsn(database)).unwrap();
    let engine = WsCborEngine::from_connection(&connection).await.unwrap();
    SurrealDB::new(SurrealClient::new(Box::new(engine), None, None))
}

struct SurrealTx {
    db: SurrealDB,
    /// The same database through its own `connect`: its own identity, so a
    /// second participant in one transaction.
    other: SurrealDB,
    next: AtomicUsize,
}

impl SurrealTx {
    async fn new() -> Self {
        let database = format!("vista_tx_{}", Uuid::new_v4().simple());
        Self {
            db: connect(&database).await,
            other: connect(&database).await,
            next: AtomicUsize::new(0),
        }
    }

    /// Without transactions: wrapped Vistas over it write directly.
    async fn bare() -> Self {
        let database = format!("vista_tx_{}", Uuid::new_v4().simple());
        let db = connect_bare(&database).await;
        Self {
            other: db.clone(),
            db,
            next: AtomicUsize::new(0),
        }
    }

    /// Names of two new, empty tables. Both refuse a record whose `n` is
    /// missing or negative, the contract's `CHECK (n >= 0)`.
    async fn create(&self) -> (String, String) {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let (a, b) = (format!("a_{n}"), format!("b_{n}"));
        let schema = format!(
            "DEFINE TABLE {a} SCHEMAFULL; \
             DEFINE FIELD n ON {a} TYPE int ASSERT $value >= 0; \
             DEFINE TABLE {b} SCHEMAFULL; \
             DEFINE FIELD a_id ON {b} TYPE option<string>; \
             DEFINE FIELD n ON {b} TYPE int ASSERT $value >= 0;"
        );
        for statement in schema.split_terminator(';') {
            self.db
                .execute(&surreal_expr!(statement.trim()))
                .await
                .unwrap();
        }
        (a, b)
    }
}

fn tables(db: SurrealDB, (a, b): (String, String)) -> TxTables {
    let factory = db.vista_factory();
    TxTables::new(
        factory.from_table(table_a(db.clone(), &a, &b)).unwrap(),
        factory.from_table(table_b(db, &b)).unwrap(),
    )
}

fn table_a(db: SurrealDB, a: &str, b: &str) -> Table<SurrealDB, EmptyEntity> {
    let target = db.clone();
    let b = b.to_string();
    Table::<SurrealDB, EmptyEntity>::new(a, db)
        .with_id_column("id")
        .with_column_of::<i64>("n")
        .with_many("b", "a_id", move |_| table_b(target.clone(), &b))
}

fn table_b(db: SurrealDB, b: &str) -> Table<SurrealDB, EmptyEntity> {
    Table::<SurrealDB, EmptyEntity>::new(b, db)
        .with_id_column("id")
        .with_column_of::<String>("a_id")
        .with_column_of::<i64>("n")
}

#[async_trait]
impl TxFixture for SurrealTx {
    async fn fresh(&self) -> TxTables {
        let names = self.create().await;
        tables(self.db.clone(), names)
    }

    /// Every clone of a `SurrealDB` already shares one connection.
    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }

    /// Optimistic: crossed writers both get through their writes, and the
    /// second to commit fails on the conflict.
    fn detects_deadlocks(&self) -> bool {
        true
    }

    fn supports_transactions(&self) -> bool {
        self.db.supports_transactions()
    }

    async fn fresh_other(&self) -> Option<TxTables> {
        let names = self.create().await;
        Some(tables(self.other.clone(), names))
    }
}

#[tokio::test]
async fn surreal_transactions_hold_the_contract() {
    contract::check_transactions(&SurrealTx::new().await).await
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn surreal_atomic_holds_the_contract() {
    contract::check_atomic_rhai(&SurrealTx::new().await).await
}

/// A client that can't open a connection of its own writes directly, and
/// the commit reports its table unprotected.
#[tokio::test]
async fn surreal_without_transactions_is_direct() {
    let fx = SurrealTx::bare().await;
    assert!(!fx.db.supports_transactions());
    contract::check_transactions(&fx).await
}

fn rec(n: i64) -> Record<CborValue> {
    [("n".to_string(), CborValue::Integer(n.into()))]
        .into_iter()
        .collect()
}

/// Review focus 4, interactive mode: a write inside the block is read back
/// by another handle wrapped into the same block, while a read outside
/// doesn't see it until the commit.
#[tokio::test]
async fn a_block_reads_its_own_write() {
    let body = async {
        let fx = SurrealTx::new().await;
        let t = fx.fresh().await;
        let tx = Transaction::new();
        tx.wrap(&t.a).insert_value("x", &rec(1)).await.unwrap();

        let reader = tx.wrap(&t.a);
        let listed = reader.list_values().await.unwrap();
        assert_eq!(listed.len(), 1, "the block lists {listed:?}");
        assert_eq!(reader.count().await.unwrap(), 1);
        assert_eq!(t.a.count().await.unwrap(), 0, "the write leaked out");

        tx.commit().await.unwrap();
        assert_eq!(t.a.count().await.unwrap(), 1);
    };
    timeout(Duration::from_secs(4), body)
        .await
        .expect("the read-own-write test ran out of time");
}
