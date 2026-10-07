//! The write contract on MySQL: confined, idempotent, invariant-filling.

use vantage_dataset::contract::{self, Fixture, OpFixture};
use vantage_dataset::prelude::*;
use vantage_sql::mysql::operation::MysqlOperation;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

/// A fresh `item_<test>` table, so tests running in parallel don't share rows.
async fn item(test: &str) -> Table<MysqlDB, EmptyEntity> {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let name = format!("item_{test}");
    sqlx::query(&format!("DROP TABLE IF EXISTS `{name}`"))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE `{name}` (id VARCHAR(64) PRIMARY KEY, name TEXT, parent TEXT, price BIGINT)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    Table::new(&name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
}

fn rec(pairs: &[(&str, AnyMysqlType)]) -> Record<AnyMysqlType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

async fn fixture(test: &str) -> (Table<MysqlDB, EmptyEntity>, Table<MysqlDB, EmptyEntity>) {
    let all = item(test).await;
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
            let (all, set) = fixture(stringify!($name)).await;
            $check(&Fixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text: |s| AnyMysqlType::from(s.to_string()),
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
    let (all, set) = fixture("where_eq_fills_on_insert").await;
    set.insert_value("n9", &rec(&[("name", "z".into())]))
        .await
        .unwrap();
    assert_eq!(
        all.get_value("n9").await.unwrap().unwrap()["parent"],
        AnyMysqlType::from("p1".to_string())
    );
}

#[tokio::test]
async fn operator_condition_keeps_rows_in_set() {
    let all = item("operator_condition_keeps_rows_in_set").await;
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
        text: |s| AnyMysqlType::from(s.to_string()),
        int: |n| AnyMysqlType::from(n),
    })
    .await;
}
