//! The write contract on SurrealDB: confined, idempotent, invariant-filling.

use std::sync::{Arc, Mutex};

use vantage_dataset::contract::{self, Fixture, OpFixture};
use vantage_dataset::prelude::*;
use vantage_surrealdb::operation::SurrealOperation;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::prelude::Hook;
use vantage_table::table::{AfterCx, Phase, Table, after_hook, before_hook_sync_cx};
use vantage_types::{EmptyEntity, Record};

async fn get_db() -> SurrealDB {
    let dsn = format!(
        "cbor://root:root@localhost:8000/bakery/safe_writes_{}",
        uuid::Uuid::new_v4().simple()
    );
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

type Seen = Arc<
    Mutex<
        Vec<(
            Option<Thing>,
            Option<AnySurrealType>,
            Option<AnySurrealType>,
        )>,
    >,
>;

/// `in1` is named `a` and `out1` `b`. A patch names `in1` `z` and a replace
/// names `out1` `y`. Both update hooks see the id and the row as stored
/// before the write, next to the written name.
#[tokio::test]
async fn update_hooks_see_the_stored_row_on_patch_and_replace() {
    let before = Seen::default();
    let after = Seen::default();
    let before_log = before.clone();
    let after_log = after.clone();
    let (all, _) = fixture().await;
    let hooked = all
        .with_hook(Hook::BeforeUpdate(
            Phase::Validate,
            before_hook_sync_cx::<SurrealDB>(move |cx| {
                before_log.lock().unwrap().push((
                    cx.id.cloned(),
                    cx.stored.and_then(|row| row.get("name").cloned()),
                    cx.record.get("name").cloned(),
                ));
                Ok(())
            }),
        ))
        .with_hook(Hook::AfterUpdate(after_hook(
            move |cx: AfterCx<'_, SurrealDB>| {
                let entry = (
                    Some(cx.id.clone()),
                    cx.before.and_then(|row| row.get("name").cloned()),
                    cx.record.get("name").cloned(),
                );
                let log = after_log.clone();
                Box::pin(async move {
                    log.lock().unwrap().push(entry);
                    Ok(())
                })
            },
        )));
    let in1 = Thing::new("item", "in1");
    let out1 = Thing::new("item", "out1");
    hooked
        .patch_value(in1.clone(), &rec(&[("name", text("z"))]))
        .await
        .unwrap();
    hooked
        .replace_value(
            out1.clone(),
            &rec(&[("name", text("y")), ("parent", text("p2"))]),
        )
        .await
        .unwrap();

    let expected = vec![
        (Some(in1), Some(text("a")), Some(text("z"))),
        (Some(out1), Some(text("b")), Some(text("y"))),
    ];
    assert_eq!(*before.lock().unwrap(), expected);
    assert_eq!(*after.lock().unwrap(), expected);
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
