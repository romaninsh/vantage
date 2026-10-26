//! Writes over a connection that signs in as a VIEWER, which SurrealDB
//! refuses with an empty result instead of an error.

use std::sync::{Arc, Mutex};

use vantage_dataset::prelude::*;
use vantage_expressions::Expression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_surrealdb::operation::SurrealOperation;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::prelude::Hook;
use vantage_table::table::{AfterCx, Table, after_hook};
use vantage_types::{EmptyEntity, Record};

async fn connect(dsn: &str) -> SurrealDB {
    let client = surreal_client::SurrealConnection::dsn(dsn)
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
}

fn text(s: &str) -> AnySurrealType {
    AnySurrealType::from(s.to_string())
}

fn rec(pairs: &[(&str, &str)]) -> Record<AnySurrealType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), text(v)))
        .collect()
}

/// A fresh database holding `item:in1`, as root and as a VIEWER of it.
async fn fixture() -> (Table<SurrealDB, EmptyEntity>, Table<SurrealDB, EmptyEntity>) {
    let name = format!("refused_writes_{}", uuid::Uuid::new_v4().simple());
    let root = connect(&format!("cbor://root:root@localhost:8000/bakery/{name}")).await;
    root.execute(&Expression::new(
        "DEFINE USER viewer ON DATABASE PASSWORD 'viewer' ROLES VIEWER",
        vec![],
    ))
    .await
    .unwrap();
    let all = item(root);
    all.insert_value(
        Thing::new("item", "in1"),
        &rec(&[("name", "a"), ("parent", "p1")]),
    )
    .await
    .unwrap();
    let viewer = connect(&format!(
        "cbor://viewer:viewer@localhost:8000/bakery/{name}?auth=database"
    ))
    .await;
    (all, item(viewer))
}

fn assert_no_effect(err: vantage_core::VantageError, expected: &str) {
    assert!(err.message().contains(expected), "unexpected error: {err}");
}

#[tokio::test]
async fn refused_delete_errors_and_skips_after_hook() {
    let (all, viewer) = fixture().await;
    let ran = Arc::new(Mutex::new(0));
    let count = ran.clone();
    let hooked = viewer.with_hook(Hook::AfterDelete(after_hook(
        move |_cx: AfterCx<'_, SurrealDB>| {
            *count.lock().unwrap() += 1;
            Box::pin(async { Ok(()) })
        },
    )));
    let err = hooked.delete(Thing::new("item", "in1")).await.unwrap_err();
    assert_no_effect(
        err,
        "delete of `item:in1` had no effect: the connection may not delete it",
    );
    assert_eq!(*ran.lock().unwrap(), 0);
    assert!(
        all.get_value(Thing::new("item", "in1"))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn delete_of_missing_row_stays_a_no_op() {
    let (all, viewer) = fixture().await;
    viewer.delete(Thing::new("item", "nope")).await.unwrap();
    all.delete(Thing::new("item", "nope")).await.unwrap();
    all.delete(Thing::new("item", "in1")).await.unwrap();
    assert!(
        all.get_value(Thing::new("item", "in1"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn refused_delete_all_errors() {
    let (all, viewer) = fixture().await;
    let set = viewer
        .clone()
        .with_condition(SurrealOperation::eq(&viewer["parent"], text("p1")));
    let err = set.delete_all().await.unwrap_err();
    assert_no_effect(err, "left rows in place");
    assert!(
        all.get_value(Thing::new("item", "in1"))
            .await
            .unwrap()
            .is_some()
    );

    // An empty set has nothing to refuse.
    let empty = viewer
        .clone()
        .with_condition(SurrealOperation::eq(&viewer["parent"], text("p9")));
    empty.delete_all().await.unwrap();
}

#[tokio::test]
async fn refused_insert_errors() {
    let (_all, viewer) = fixture().await;
    let err = viewer
        .insert_value(Thing::new("item", "n2"), &rec(&[("name", "q")]))
        .await
        .unwrap_err();
    assert_no_effect(
        err,
        "insert of `item:n2` had no effect: the connection may not create it",
    );
    let err = viewer
        .insert_return_id_value(&rec(&[("name", "q")]))
        .await
        .unwrap_err();
    assert_no_effect(err, "insert into `item` had no effect");
}

#[tokio::test]
async fn refused_patch_and_replace_error() {
    let (all, viewer) = fixture().await;
    let id = Thing::new("item", "in1");
    let err = viewer
        .patch_value(id.clone(), &rec(&[("name", "z")]))
        .await
        .unwrap_err();
    assert_no_effect(err, "patch of `item:in1` had no effect");
    let err = viewer
        .replace_value(id.clone(), &rec(&[("name", "z"), ("parent", "p1")]))
        .await
        .unwrap_err();
    assert_no_effect(err, "replace of `item:in1` had no effect");
    assert_eq!(all.get_value(id).await.unwrap().unwrap()["name"], text("a"));

    // A patch of a row that isn't there is still not-found.
    let err = viewer
        .patch_value(Thing::new("item", "nope"), &rec(&[("name", "z")]))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), vantage_core::ErrorKind::NotFound);
}
