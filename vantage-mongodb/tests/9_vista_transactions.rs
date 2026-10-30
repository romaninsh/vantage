//! `vantage_vista::contract::check_transactions` and `check_atomic_rhai` over
//! a MongoDB replica set (`MONGODB_URL`, e.g.
//! `mongodb://localhost:27017/?directConnection=true`). Each fixture works in
//! a randomised database of its own; every `fresh` call makes its own
//! numbered collections there.

#![cfg(feature = "vista")]

use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use futures_util::FutureExt;
use indexmap::IndexMap;
use tokio::time::timeout;
use vantage_core::ErrorKind;
use vantage_dataset::prelude::*;
use vantage_mongodb::MongoDB;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::Transaction;
use vantage_vista::contract::{self, TxFixture, TxTables};

const FAILED_STATEMENT: &str = "a statement failed inside the transaction; rolled back";

fn mongo_url() -> String {
    std::env::var("MONGODB_URL")
        .unwrap_or_else(|_| "mongodb://localhost:27017/?directConnection=true".into())
}

/// A pool of one connection on the same server.
fn single_connection_url() -> String {
    let url = mongo_url();
    let joiner = if url.contains('?') { '&' } else { '?' };
    format!("{url}{joiner}maxPoolSize=1")
}

struct MongoTx {
    db: MongoDB,
    /// The same database through its own `connect`: its own client and
    /// identity, so a second participant in one transaction.
    other: MongoDB,
    name: String,
    next: AtomicUsize,
}

impl MongoTx {
    async fn new(prefix: &str) -> Self {
        let name = format!("vantage_{prefix}_{}", ObjectId::new().to_hex());
        Self {
            db: MongoDB::connect(&mongo_url(), &name).await.unwrap(),
            other: MongoDB::connect(&mongo_url(), &name).await.unwrap(),
            name,
            next: AtomicUsize::new(0),
        }
    }

    /// Names of two new, empty collections. Both refuse a document whose `n`
    /// is missing or negative, the contract's `CHECK (n >= 0)`.
    async fn create(&self) -> (String, String) {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let (a, b) = (format!("a_{n}"), format!("b_{n}"));
        let validator = doc! { "$jsonSchema": {
            "bsonType": "object",
            "required": ["n"],
            "properties": { "n": { "bsonType": ["int", "long"], "minimum": 0 } },
        } };
        for name in [&a, &b] {
            self.db
                .database()
                .create_collection(name)
                .validator(validator.clone())
                .await
                .unwrap();
        }
        (a, b)
    }

    async fn teardown(&self) {
        self.db
            .database()
            .drop()
            .await
            .unwrap_or_else(|e| eprintln!("drop {}: {e}", self.name));
    }
}

fn tables(db: MongoDB, (a, b): (String, String)) -> TxTables {
    let factory = db.vista_factory();
    TxTables::new(
        factory.from_table(table_a(db.clone(), &a, &b)).unwrap(),
        factory.from_table(table_b(db, &b)).unwrap(),
    )
}

/// The contract's rows name their id `id`; the documents keep it in `_id`.
fn table_a(db: MongoDB, a: &str, b: &str) -> Table<MongoDB, EmptyEntity> {
    let target = db.clone();
    let b = b.to_string();
    Table::<MongoDB, EmptyEntity>::new(a, db)
        .with_id_column("id")
        .with_column_of::<i64>("n")
        .with_many("b", "a_id", move |_| table_b(target.clone(), &b))
}

fn table_b(db: MongoDB, b: &str) -> Table<MongoDB, EmptyEntity> {
    Table::<MongoDB, EmptyEntity>::new(b, db)
        .with_id_column("id")
        .with_column_of::<String>("a_id")
        .with_column_of::<i64>("n")
}

#[async_trait]
impl TxFixture for MongoTx {
    async fn fresh(&self) -> TxTables {
        let names = self.create().await;
        tables(self.db.clone(), names)
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        let names = self.create().await;
        let db = MongoDB::connect(&single_connection_url(), &self.name)
            .await
            .unwrap();
        tables(db, names)
    }

    fn detects_deadlocks(&self) -> bool {
        true
    }

    /// A write to a document another transaction holds fails at once with a
    /// `WriteConflict`; it neither waits nor deadlocks.
    fn conflicts_fail_fast(&self) -> bool {
        true
    }

    async fn fresh_other(&self) -> Option<TxTables> {
        let names = self.create().await;
        Some(tables(self.other.clone(), names))
    }
}

/// Runs `check` on a new fixture and drops its database, whether the check
/// passes or panics: a server with a low open-files limit runs out of them
/// on leftover collections.
async fn with_fixture(prefix: &str, check: impl AsyncFnOnce(&MongoTx)) {
    let fx = MongoTx::new(prefix).await;
    let ran = AssertUnwindSafe(check(&fx)).catch_unwind().await;
    fx.teardown().await;
    if let Err(panic) = ran {
        resume_unwind(panic);
    }
}

#[tokio::test]
async fn mongo_transactions_hold_the_contract() {
    with_fixture("contract_tx", async |fx| {
        contract::check_transactions(fx).await
    })
    .await
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn mongo_atomic_holds_the_contract() {
    with_fixture("contract_atomic", async |fx| {
        contract::check_atomic_rhai(fx).await
    })
    .await
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// An import through a wrapped Vista opens no transaction of its own: its rows
/// land with the commit, not before.
#[tokio::test]
async fn mongo_import_joins_the_transaction() {
    with_fixture("import", async |fx| {
        timeout(Duration::from_secs(4), import_joins_the_transaction(fx))
            .await
            .expect("the import test ran out of time")
    })
    .await
}

async fn import_joins_the_transaction(fx: &MongoTx) {
    let t = fx.fresh().await;
    let rows: IndexMap<String, Record<CborValue>> = [("x", 1), ("z", 2)]
        .into_iter()
        .map(|(id, n)| (id.to_string(), rec(&[("n", int(n))])))
        .collect();
    let tx = Transaction::new();
    assert_eq!(tx.wrap(&t.a).import_values(&rows).await.unwrap(), 2);
    assert!(t.a.get_value("x").await.unwrap().is_none());
    tx.commit().await.unwrap();
    assert!(t.a.get_value("x").await.unwrap().is_some());
    assert!(t.a.get_value("z").await.unwrap().is_some());
}

/// Two transactions patch the same document. The second write conflicts: it
/// errs at once, nothing retries it, and its transaction's other write rolls
/// back with it. The first transaction commits untouched.
#[tokio::test]
async fn mongo_write_conflict_rolls_back() {
    with_fixture("write_conflict", async |fx| {
        timeout(Duration::from_secs(4), write_conflict_rolls_back(fx))
            .await
            .expect("the write-conflict test ran out of time")
    })
    .await
}

async fn write_conflict_rolls_back(fx: &MongoTx) {
    let t = fx.fresh().await;
    t.a.insert_value("x", &rec(&[("n", int(1))])).await.unwrap();
    let (tx1, tx2) = (Transaction::new(), Transaction::new());
    let (a1, a2, b2) = (tx1.wrap(&t.a), tx2.wrap(&t.a), tx2.wrap(&t.b));

    a1.patch_value("x", &rec(&[("n", int(2))])).await.unwrap();
    let y = rec(&[("a_id", CborValue::Text("x".into())), ("n", int(1))]);
    b2.insert_value("y", &y).await.unwrap();
    let started = Instant::now();
    let conflict = a2
        .patch_value("x", &rec(&[("n", int(3))]))
        .await
        .unwrap_err();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the conflicting write waited {:?}",
        started.elapsed()
    );
    assert!(
        conflict.to_string().contains("WriteConflict")
            && matches!(conflict.kind(), ErrorKind::Conflict),
        "the conflicting write errs with `{conflict}`"
    );

    let failed = tx2.commit().await.unwrap_err();
    assert!(
        failed.to_string().contains(FAILED_STATEMENT),
        "the conflicted commit errs with `{failed}`"
    );
    tx1.commit().await.unwrap();

    let x = t.a.get_value("x").await.unwrap().unwrap();
    assert_eq!(x.get("n"), Some(&int(2)));
    assert!(t.b.get_value("y").await.unwrap().is_none());
}
