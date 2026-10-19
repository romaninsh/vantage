#![cfg(feature = "mocks")]

//! Hooks built from synchronous closures: `before_hook_sync` and
//! `before_delete_hook_sync`.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use vantage_dataset::prelude::*;
use vantage_table::prelude::*;
use vantage_table::table::{Phase, before_delete_hook_sync, before_hook_sync};
use vantage_types::{EmptyEntity, Record};

type MockTable = Table<MockTableSource, EmptyEntity>;

fn rec(value: Value) -> Record<Value> {
    Record::from(value)
}

async fn table(rows: Vec<Value>) -> MockTable {
    let src = MockTableSource::new().with_data("t", rows).await;
    MockTable::new("t", src).with_id_column("id")
}

#[tokio::test]
async fn a_sync_before_insert_refuses() {
    let table = table(vec![]).await.with_hook(Hook::BeforeInsert(
        Phase::Validate,
        before_hook_sync::<MockTableSource>(|record, _context| {
            if record.get("name") == Some(&json!("bad")) {
                return Err(vantage_core::error!("viewers change nothing"));
            }
            Ok(())
        }),
    ));
    let err = table
        .insert_value("1".to_string(), &rec(json!({"name": "bad"})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "viewers change nothing");
    assert!(table.get_value("1".to_string()).await.unwrap().is_none());

    table
        .insert_value("2".to_string(), &rec(json!({"name": "good"})))
        .await
        .unwrap();
    assert!(table.get_value("2".to_string()).await.unwrap().is_some());
}

#[tokio::test]
async fn a_sync_before_save_runs_on_insert_and_update() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let table = table(vec![]).await.with_hook(Hook::BeforeSave(
        Phase::Populate,
        before_hook_sync::<MockTableSource>(move |record, _context| {
            log.lock().unwrap().push(record["name"].clone());
            record.insert("stamped".to_string(), json!(true));
            Ok(())
        }),
    ));
    table
        .insert_value("1".to_string(), &rec(json!({"name": "a"})))
        .await
        .unwrap();
    table
        .patch_value("1".to_string(), &rec(json!({"name": "b"})))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![json!("a"), json!("b")]);
    let row = table.get_value("1".to_string()).await.unwrap().unwrap();
    assert_eq!(row["stamped"], json!(true));
}

#[tokio::test]
async fn a_sync_before_delete_refuses() {
    let table = table(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeDelete(
            before_delete_hook_sync::<MockTableSource>(|id, _context| {
                if id == "1" {
                    return Err(vantage_core::error!("deletion not allowed"));
                }
                Ok(())
            }),
        ));
    let err = table.delete("1".to_string()).await.unwrap_err();
    assert_eq!(err.message(), "deletion not allowed");
    assert!(table.get_value("1".to_string()).await.unwrap().is_some());
}
