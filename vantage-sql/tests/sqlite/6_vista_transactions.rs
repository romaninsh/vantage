//! `vantage_vista::contract::check_transactions` over file-backed SQLite: an
//! in-memory database is private to one connection, and the probes need a
//! second one to read "outside" the transaction.

#![cfg(feature = "vista")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use tempfile::TempDir;
use tokio::time::timeout;
use vantage_dataset::traits::WritableValueSet;
use vantage_sql::sqlite::SqliteDB;
use vantage_table::table::{AfterFn, Hook, Table};
use vantage_table::write_context::WriteContext;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::contract::{self, TxFixture, TxTables};
use vantage_vista::{Transaction, Vista};

struct SqliteTx {
    dir: TempDir,
    next: AtomicUsize,
}

impl SqliteTx {
    async fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            next: AtomicUsize::new(0),
        }
    }

    /// A new database file holding the two empty tables.
    async fn create(&self) -> SqliteConnectOptions {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let options = SqliteConnectOptions::new()
            .filename(self.dir.path().join(format!("tx{n}.sqlite")))
            .create_if_missing(true);
        let db = SqliteDB::new(SqlitePool::connect_with(options.clone()).await.unwrap());
        for sql in [
            "CREATE TABLE a (id TEXT PRIMARY KEY, n INTEGER NOT NULL CHECK (n >= 0))",
            "CREATE TABLE b (id TEXT PRIMARY KEY, a_id TEXT, n INTEGER NOT NULL CHECK (n >= 0))",
        ] {
            sqlx::query(sql).execute(db.pool()).await.unwrap();
        }
        options
    }
}

fn tables(db: SqliteDB) -> TxTables {
    let factory = db.vista_factory();
    TxTables::new(
        factory.from_table(table_a(db.clone())).unwrap(),
        factory.from_table(table_b(db)).unwrap(),
    )
}

fn table_a(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    let target = db.clone();
    Table::<SqliteDB, EmptyEntity>::new("a", db)
        .with_id_column("id")
        .with_column_of::<i64>("n")
        .with_many("b", "a_id", move |_| table_b(target.clone()))
}

fn table_b(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("b", db)
        .with_id_column("id")
        .with_column_of::<String>("a_id")
        .with_column_of::<i64>("n")
}

#[async_trait]
impl TxFixture for SqliteTx {
    async fn fresh(&self) -> TxTables {
        let options = self.create().await;
        tables(SqliteDB::new(
            SqlitePool::connect_with(options).await.unwrap(),
        ))
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        let options = self.create().await;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        tables(SqliteDB::new(pool))
    }

    fn detects_deadlocks(&self) -> bool {
        false
    }

    fn supports_lock(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn sqlite_transactions_hold_the_contract() {
    contract::check_transactions(&SqliteTx::new().await).await
}

const DEADLINE: Duration = Duration::from_secs(2);

/// Table `a` on a new database file, with `hook` as its after-insert hook.
/// The fixture owns the file's directory.
async fn hooked(hook: AfterFn<SqliteDB>) -> (SqliteTx, Vista) {
    let fx = SqliteTx::new().await;
    let db = SqliteDB::new(SqlitePool::connect_with(fx.create().await).await.unwrap());
    let table = table_a(db.clone()).with_hook(Hook::AfterInsert(hook));
    (fx, db.vista_factory().from_table(table).unwrap())
}

fn rec(n: i64) -> Record<CborValue> {
    [("n".to_string(), CborValue::Integer(n.into()))]
        .into_iter()
        .collect()
}

#[tokio::test]
async fn after_hooks_wait_for_commit() {
    let body = async {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let hook: AfterFn<SqliteDB> = Arc::new(move |_cx| {
            let counted = counted.clone();
            Box::pin(async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        let (_fx, a) = hooked(hook).await;

        let tx = Transaction::new();
        tx.wrap(&a).insert_value("x", &rec(1)).await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the hook ran before commit"
        );
        tx.commit().await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the hook didn't run on commit"
        );

        let tx = Transaction::new();
        tx.wrap(&a).insert_value("z", &rec(1)).await.unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the hook ran after rollback"
        );
    };
    timeout(DEADLINE, body).await.expect("a hooked insert hung");
}

#[tokio::test]
async fn deferred_hook_sees_the_write_context() {
    let body = async {
        let seen = Arc::new(Mutex::new(None));
        let record = seen.clone();
        let hook: AfterFn<SqliteDB> = Arc::new(move |_cx| {
            let record = record.clone();
            Box::pin(async move {
                *record.lock().unwrap() = WriteContext::current().and_then(|c| c.group);
                Ok(())
            })
        });
        let (_fx, a) = hooked(hook).await;

        let tx = Transaction::new();
        let wrapped = tx.wrap(&a);
        WriteContext::new()
            .with_group("g")
            .scope(wrapped.insert_value("x", &rec(1)))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(seen.lock().unwrap().as_deref(), Some("g"));
    };
    timeout(DEADLINE, body).await.expect("a hooked insert hung");
}
