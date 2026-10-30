//! `vantage_vista::contract::check_transactions` over MySQL, and an insert
//! without an id inside a transaction.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.
//! Every `fresh` call makes its own suffixed tables.

#![cfg(feature = "vista")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use sqlx::mysql::MySqlPoolOptions;
use tokio::time::timeout;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet};
use vantage_sql::mysql::MysqlDB;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::contract::{self, TxFixture, TxTables};
use vantage_vista::{Transaction, Vista};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

struct MysqlTx {
    db: MysqlDB,
    next: AtomicUsize,
}

impl MysqlTx {
    async fn new() -> Self {
        Self {
            db: MysqlDB::connect(MYSQL_URL).await.unwrap(),
            next: AtomicUsize::new(0),
        }
    }

    /// Names of two new, empty tables.
    async fn create(&self) -> (String, String) {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let (a, b) = (format!("contract_tx_a_{n}"), format!("contract_tx_b_{n}"));
        for sql in [
            format!("DROP TABLE IF EXISTS `{a}`"),
            format!("DROP TABLE IF EXISTS `{b}`"),
            format!(
                "CREATE TABLE `{a}` (id VARCHAR(255) PRIMARY KEY, n BIGINT NOT NULL CHECK (n >= 0))"
            ),
            format!(
                "CREATE TABLE `{b}` (id VARCHAR(255) PRIMARY KEY, a_id VARCHAR(255), n BIGINT NOT NULL CHECK (n >= 0))"
            ),
        ] {
            sqlx::query(&sql).execute(self.db.pool()).await.unwrap();
        }
        (a, b)
    }
}

async fn pool_of_one() -> MysqlDB {
    let pool = MySqlPoolOptions::new()
        .max_connections(1)
        .connect(MYSQL_URL)
        .await
        .unwrap();
    MysqlDB::new(pool)
}

fn tables(db: MysqlDB, (a, b): (String, String)) -> TxTables {
    let factory = db.vista_factory();
    TxTables {
        a: factory.from_table(table_a(db.clone(), &a, &b)).unwrap(),
        b: factory.from_table(table_b(db, &b)).unwrap(),
    }
}

fn table_a(db: MysqlDB, a: &str, b: &str) -> Table<MysqlDB, EmptyEntity> {
    let target = db.clone();
    let b = b.to_string();
    Table::<MysqlDB, EmptyEntity>::new(a, db)
        .with_id_column("id")
        .with_column_of::<i64>("n")
        .with_many("b", "a_id", move |_| table_b(target.clone(), &b))
}

fn table_b(db: MysqlDB, b: &str) -> Table<MysqlDB, EmptyEntity> {
    Table::<MysqlDB, EmptyEntity>::new(b, db)
        .with_id_column("id")
        .with_column_of::<String>("a_id")
        .with_column_of::<i64>("n")
}

#[async_trait]
impl TxFixture for MysqlTx {
    async fn fresh(&self) -> TxTables {
        let names = self.create().await;
        tables(self.db.clone(), names)
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        let names = self.create().await;
        tables(pool_of_one().await, names)
    }

    fn detects_deadlocks(&self) -> bool {
        true
    }

    fn supports_lock(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn mysql_transactions_hold_the_contract() {
    contract::check_transactions(&MysqlTx::new().await).await
}

const DEADLINE: Duration = Duration::from_secs(2);

/// `table`, made anew as `columns`, as a vista on `db`.
async fn single_table(db: &MysqlDB, table: &str, columns: &str) -> Vista {
    for sql in [
        format!("DROP TABLE IF EXISTS `{table}`"),
        format!("CREATE TABLE `{table}` ({columns})"),
    ] {
        sqlx::query(&sql).execute(db.pool()).await.unwrap();
    }
    vista_of(db, table)
}

fn vista_of(db: &MysqlDB, table: &str) -> Vista {
    db.vista_factory()
        .from_table(
            Table::<MysqlDB, EmptyEntity>::new(table, db.clone())
                .with_id_column("id")
                .with_column_of::<i64>("n"),
        )
        .unwrap()
}

fn rec(n: i64) -> Record<CborValue> {
    [("n".to_string(), CborValue::Integer(n.into()))]
        .into_iter()
        .collect()
}

/// `INSERT` and `SELECT LAST_INSERT_ID()` both run on the transaction's
/// connection: on a pool of one, a second connection would never come.
#[tokio::test]
async fn mysql_insert_without_id_runs_on_the_transaction() {
    let body = async {
        let table = "contract_tx_auto";
        let db = pool_of_one().await;
        let vista = single_table(
            &db,
            table,
            "id BIGINT AUTO_INCREMENT PRIMARY KEY, n BIGINT NOT NULL",
        )
        .await;

        let tx = Transaction::new();
        let wrapped = tx.wrap(&vista);
        // Begins the transaction, which now holds the pool's one connection.
        wrapped.count().await.unwrap();
        let id = wrapped.insert_return_id_value(&rec(1)).await.unwrap();
        tx.commit().await.unwrap();

        assert!(
            vista.get_value(&id).await.unwrap().is_some(),
            "{table}/{id} is not found outside after commit"
        );
    };
    timeout(DEADLINE, body)
        .await
        .expect("a wrapped insert without an id hung");
}

/// A wrapped import writes on the block's transaction: nothing of it shows
/// outside before commit, and rollback undoes it.
#[tokio::test]
async fn mysql_import_runs_on_the_transaction() {
    let body = async {
        let table = "contract_tx_import";
        let db = pool_of_one().await;
        let vista =
            single_table(&db, table, "id VARCHAR(255) PRIMARY KEY, n BIGINT NOT NULL").await;
        let outside = vista_of(&MysqlDB::connect(MYSQL_URL).await.unwrap(), table);
        let rows = |ids: [&str; 2]| -> IndexMap<String, Record<CborValue>> {
            ids.into_iter().map(|id| (id.to_string(), rec(1))).collect()
        };

        let tx = Transaction::new();
        let imported = tx
            .wrap(&vista)
            .import_values(&rows(["x", "z"]))
            .await
            .unwrap();
        assert_eq!(imported, 2);
        assert!(
            outside.get_value("x").await.unwrap().is_none(),
            "{table}/x is found outside before commit"
        );
        tx.commit().await.unwrap();
        for id in ["x", "z"] {
            assert!(
                outside.get_value(id).await.unwrap().is_some(),
                "{table}/{id} is not found outside after commit"
            );
        }

        let tx = Transaction::new();
        tx.wrap(&vista)
            .import_values(&rows(["p", "q"]))
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        for id in ["p", "q"] {
            assert!(
                outside.get_value(id).await.unwrap().is_none(),
                "{table}/{id} is found outside after rollback"
            );
        }
    };
    timeout(DEADLINE, body)
        .await
        .expect("a wrapped import hung");
}
