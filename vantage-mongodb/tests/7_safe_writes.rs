//! The write contract on MongoDB: confined, idempotent, invariant-filling.
//!
//! Requires a running MongoDB instance (`MONGODB_URL`, default
//! mongodb://localhost:27017); each test uses its own database.

use std::sync::{Arc, Mutex};

use bson::{Bson, doc};
use vantage_dataset::contract::{self, Fixture, OpFixture};
use vantage_dataset::prelude::*;
use vantage_mongodb::{AnyMongoType, MongoCondition, MongoDB, MongoId};
use vantage_table::prelude::Hook;
use vantage_table::table::{AfterCx, Table, after_hook};
use vantage_types::{EmptyEntity, Record};

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> MongoDB {
    let db_name = format!("vantage_test_{}", bson::oid::ObjectId::new().to_hex());
    MongoDB::connect(&mongo_url(), &db_name)
        .await
        .expect("Failed to connect to MongoDB")
}

async fn teardown(db: &MongoDB) {
    db.database()
        .drop()
        .await
        .unwrap_or_else(|e| eprintln!("Failed to drop test db: {}", e));
}

fn item(db: MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::new("item", db)
        .with_id_column("_id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
}

fn text(s: &str) -> AnyMongoType {
    AnyMongoType::from(s)
}

fn rec(pairs: &[(&str, AnyMongoType)]) -> Record<AnyMongoType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

async fn fixture() -> (
    MongoDB,
    Table<MongoDB, EmptyEntity>,
    Table<MongoDB, EmptyEntity>,
) {
    let db = setup().await;
    let all = item(db.clone());
    all.insert_value(
        MongoId::from("in1"),
        &rec(&[("name", text("a")), ("parent", text("p1"))]),
    )
    .await
    .unwrap();
    all.insert_value(
        MongoId::from("out1"),
        &rec(&[("name", text("b")), ("parent", text("p2"))]),
    )
    .await
    .unwrap();
    let set = all
        .clone()
        .with_condition(MongoCondition::Doc(doc! {"parent": "p1"}));
    (db, all, set)
}

macro_rules! mongo_check {
    ($name:ident, $check:path) => {
        #[tokio::test]
        async fn $name() {
            let (db, all, set) = fixture().await;
            $check(&Fixture {
                all: &all,
                set: &set,
                id: |s| MongoId::from(s),
                text: |s| AnyMongoType::from(s),
                detects_outside: true,
            })
            .await;
            teardown(&db).await;
        }
    };
}

mongo_check!(delete_is_confined, contract::check_delete);
mongo_check!(insert_is_confined, contract::check_insert);
mongo_check!(patch_is_confined, contract::check_patch);
mongo_check!(replace_is_confined, contract::check_replace);
mongo_check!(delete_all_is_confined, contract::check_delete_all);

#[tokio::test]
async fn get_honours_conditions() {
    let (db, _all, set) = fixture().await;
    assert!(set.get_value(MongoId::from("in1")).await.unwrap().is_some());
    assert!(
        set.get_value(MongoId::from("out1"))
            .await
            .unwrap()
            .is_none()
    );
    teardown(&db).await;
}

type Seen = Arc<Mutex<Vec<(Option<AnyMongoType>, Option<AnyMongoType>)>>>;

/// `in1` is named `a`; an update through a table with an after-update hook
/// names it `z`, and the hook sees both.
async fn assert_after_update_sees_the_former_row(patch: bool) {
    let seen = Seen::default();
    let log = seen.clone();
    let (db, all, _) = fixture().await;
    let hooked = all.with_hook(Hook::AfterUpdate(after_hook(
        move |cx: AfterCx<'_, MongoDB>| {
            let pair = (
                cx.before.and_then(|b| b.get("name").cloned()),
                cx.record.get("name").cloned(),
            );
            let log = log.clone();
            Box::pin(async move {
                log.lock().unwrap().push(pair);
                Ok(())
            })
        },
    )));
    let id = MongoId::from("in1");
    if patch {
        hooked
            .patch_value(id, &rec(&[("name", text("z"))]))
            .await
            .unwrap();
    } else {
        let full = rec(&[("name", text("z")), ("parent", text("p1"))]);
        hooked.replace_value(id, &full).await.unwrap();
    }
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(Some(text("a")), Some(text("z")))]
    );
    teardown(&db).await;
}

#[tokio::test]
async fn after_update_sees_the_former_row_on_replace() {
    assert_after_update_sees_the_former_row(false).await;
}

#[tokio::test]
async fn after_update_sees_the_former_row_on_patch() {
    assert_after_update_sees_the_former_row(true).await;
}

#[tokio::test]
async fn operator_condition_keeps_rows_in_set() {
    let db = setup().await;
    let all = item(db.clone());
    all.insert_value(
        MongoId::from("cheap"),
        &rec(&[
            ("name", text("c")),
            ("price", AnyMongoType::untyped(Bson::Int64(5))),
        ]),
    )
    .await
    .unwrap();
    all.insert_value(
        MongoId::from("dear"),
        &rec(&[
            ("name", text("d")),
            ("price", AnyMongoType::untyped(Bson::Int64(20))),
        ]),
    )
    .await
    .unwrap();
    let set = all
        .clone()
        .with_condition(MongoCondition::Doc(doc! {"price": {"$gt": 10}}));
    contract::check_operator_condition(&OpFixture {
        all: &all,
        set: &set,
        id: |s| MongoId::from(s),
        text: |s| AnyMongoType::from(s),
        int: |n| AnyMongoType::untyped(Bson::Int64(n)),
    })
    .await;
    teardown(&db).await;
}
