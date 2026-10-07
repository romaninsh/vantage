//! The write contract on SQLite: confined, idempotent, invariant-filling.

use vantage_dataset::contract::{self, Fixture, OpFixture};
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::operation::SqliteOperation;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

async fn db() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE item (id TEXT PRIMARY KEY, name TEXT, parent TEXT, price INTEGER)")
        .execute(db.pool())
        .await
        .unwrap();
    db
}

fn item(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::new("item", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
}

fn rec(pairs: &[(&str, AnySqliteType)]) -> Record<AnySqliteType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

async fn fixture() -> (Table<SqliteDB, EmptyEntity>, Table<SqliteDB, EmptyEntity>) {
    let all = item(db().await);
    all.insert_value(
        "in1",
        &rec(&[("name", "a".into()), ("parent", "p1".into())]),
    )
    .await
    .unwrap();
    all.insert_value(
        "out1",
        &rec(&[("name", "b".into()), ("parent", "p2".into())]),
    )
    .await
    .unwrap();
    let parent = all["parent"].clone();
    let set = all.clone().with_condition(parent.eq("p1"));
    (all, set)
}

macro_rules! sql_check {
    ($name:ident, $check:path) => {
        #[tokio::test]
        async fn $name() {
            let (all, set) = fixture().await;
            $check(&Fixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text: |s| AnySqliteType::from(s.to_string()),
                detects_outside: true,
            })
            .await;
        }
    };
}

sql_check!(delete_is_confined, contract::check_delete);
sql_check!(insert_is_confined, contract::check_insert);
sql_check!(patch_is_confined, contract::check_patch);
sql_check!(replace_is_confined, contract::check_replace);
sql_check!(delete_all_is_confined, contract::check_delete_all);

#[tokio::test]
async fn where_eq_fills_on_insert() {
    let (all, set) = fixture().await;
    set.insert_value("n9", &rec(&[("name", "z".into())]))
        .await
        .unwrap();
    assert_eq!(
        all.get_value("n9").await.unwrap().unwrap()["parent"],
        AnySqliteType::from("p1".to_string())
    );
}

#[tokio::test]
async fn operator_condition_keeps_rows_in_set() {
    let all = item(db().await);
    all.insert_value(
        "cheap",
        &rec(&[("name", "c".into()), ("price", 5i64.into())]),
    )
    .await
    .unwrap();
    all.insert_value(
        "dear",
        &rec(&[("name", "d".into()), ("price", 20i64.into())]),
    )
    .await
    .unwrap();
    let price = all["price"].clone();
    let set = all.clone().with_condition(price.gt(10i64));
    contract::check_operator_condition(&OpFixture {
        all: &all,
        set: &set,
        id: |s| s.to_string(),
        text: |s| AnySqliteType::from(s.to_string()),
        int: |n| AnySqliteType::from(n),
    })
    .await;
}
