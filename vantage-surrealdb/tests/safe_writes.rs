//! The write contract on SurrealDB: confined, idempotent, invariant-filling.

use std::sync::atomic::{AtomicU32, Ordering};

use vantage_dataset::contract::{self, Fixture, OpFixture};
use vantage_dataset::prelude::*;
use vantage_surrealdb::operation::SurrealOperation;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn get_db() -> SurrealDB {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dsn = format!("cbor://root:root@localhost:8000/bakery/safe_writes_{}", n);
    let client = surreal_client::SurrealConnection::dsn(&dsn)
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

fn item(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("item", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
}

fn rec(pairs: &[(&str, AnySurrealType)]) -> Record<AnySurrealType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn text(s: &str) -> AnySurrealType {
    AnySurrealType::from(s.to_string())
}

async fn fixture() -> (Table<SurrealDB, EmptyEntity>, Table<SurrealDB, EmptyEntity>) {
    let all = item(get_db().await);
    all.insert_value(
        Thing::new("item", "in1"),
        &rec(&[("name", text("a")), ("parent", text("p1"))]),
    )
    .await
    .unwrap();
    all.insert_value(
        Thing::new("item", "out1"),
        &rec(&[("name", text("b")), ("parent", text("p2"))]),
    )
    .await
    .unwrap();
    let set = all
        .clone()
        .with_condition(SurrealOperation::eq(&all["parent"], text("p1")));
    (all, set)
}

macro_rules! surreal_check {
    ($name:ident, $check:path) => {
        #[tokio::test]
        async fn $name() {
            let (all, set) = fixture().await;
            $check(&Fixture {
                all: &all,
                set: &set,
                id: |s| Thing::new("item", s),
                text: |s| AnySurrealType::from(s.to_string()),
                detects_outside: true,
            })
            .await;
        }
    };
}

surreal_check!(delete_is_confined, contract::check_delete);
surreal_check!(insert_is_confined, contract::check_insert);
surreal_check!(patch_is_confined, contract::check_patch);
surreal_check!(replace_is_confined, contract::check_replace);
surreal_check!(delete_all_is_confined, contract::check_delete_all);

#[tokio::test]
async fn where_eq_fills_on_insert() {
    let (all, set) = fixture().await;
    set.insert_value(Thing::new("item", "n9"), &rec(&[("name", text("z"))]))
        .await
        .unwrap();
    assert_eq!(
        all.get_value(Thing::new("item", "n9"))
            .await
            .unwrap()
            .unwrap()["parent"],
        text("p1")
    );
}

#[tokio::test]
async fn operator_condition_keeps_rows_in_set() {
    let all = item(get_db().await);
    all.insert_value(
        Thing::new("item", "cheap"),
        &rec(&[("name", text("c")), ("price", AnySurrealType::from(5i64))]),
    )
    .await
    .unwrap();
    all.insert_value(
        Thing::new("item", "dear"),
        &rec(&[("name", text("d")), ("price", AnySurrealType::from(20i64))]),
    )
    .await
    .unwrap();
    let set = all
        .clone()
        .with_condition(SurrealOperation::gt(&all["price"], 10i64));
    contract::check_operator_condition(&OpFixture {
        all: &all,
        set: &set,
        id: |s| Thing::new("item", s),
        text: |s| AnySurrealType::from(s.to_string()),
        int: |n| AnySurrealType::from(n),
    })
    .await;
}
