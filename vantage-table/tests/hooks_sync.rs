#![cfg(feature = "mocks")]

//! Hooks built from synchronous closures (`before_hook_sync`,
//! `before_hook_sync_cx`, `before_delete_hook_sync`), and the full row a
//! before-update hook sees: the id and the stored row next to the record.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use vantage_dataset::prelude::*;
use vantage_table::prelude::*;
use vantage_table::table::{
    AfterCx, BeforeCx, Phase, SelectCx, after_hook, before_delete_hook_sync, before_hook,
    before_hook_sync, before_hook_sync_cx, select_hook,
};
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

// --- the full row on update --------------------------------------------------

type Seen = Arc<Mutex<Vec<(Option<String>, Option<Value>, Value)>>>;

/// A before-update hook recording `(id, stored row, record)` for each write.
fn seeing(seen: Seen) -> Hook<MockTableSource> {
    Hook::BeforeUpdate(
        Phase::Validate,
        before_hook(move |cx: BeforeCx<'_, MockTableSource>| {
            seen.lock().unwrap().push((
                cx.id.cloned(),
                cx.stored.map(|row| Value::from(row.clone())),
                Value::from(cx.record.clone()),
            ));
            Box::pin(async { Ok(()) })
        }),
    )
}

fn stored_row() -> Value {
    json!({"id": "1", "name": "a", "environment": "dev"})
}

#[tokio::test]
async fn a_before_update_hook_on_a_patch_sees_the_stored_row_and_the_id() {
    let seen = Seen::default();
    let table = table(vec![stored_row()])
        .await
        .with_hook(seeing(seen.clone()));
    table
        .patch_value("1".to_string(), &rec(json!({"name": "z"})))
        .await
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [(
            Some("1".to_string()),
            Some(stored_row()),
            json!({"name": "z"})
        )]
    );
}

#[tokio::test]
async fn a_before_update_hook_on_a_replace_sees_the_stored_row_and_the_id() {
    let seen = Seen::default();
    let table = table(vec![stored_row()])
        .await
        .with_hook(seeing(seen.clone()));
    table
        .replace_value("1".to_string(), &rec(json!({"name": "z"})))
        .await
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [(
            Some("1".to_string()),
            Some(stored_row()),
            json!({"name": "z"})
        )]
    );
}

#[tokio::test]
async fn a_before_update_hook_on_a_missing_row_sees_no_stored_row() {
    let seen = Seen::default();
    let table = table(vec![]).await.with_hook(seeing(seen.clone()));
    table
        .replace_value("9".to_string(), &rec(json!({"name": "z"})))
        .await
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [(Some("9".to_string()), None, json!({"name": "z"}))]
    );
}

#[tokio::test]
async fn a_before_insert_hook_sees_no_id_or_stored_row() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let table = table(vec![]).await.with_hook(Hook::BeforeSave(
        Phase::Validate,
        before_hook_sync_cx::<MockTableSource>(move |cx| {
            log.lock()
                .unwrap()
                .push((cx.id.cloned(), cx.stored.is_some()));
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
    assert_eq!(
        *seen.lock().unwrap(),
        [(None, false), (Some("1".to_string()), true)]
    );
}

#[tokio::test]
async fn a_sync_cx_hook_refuses_on_the_stored_row() {
    let table = table(vec![stored_row()])
        .await
        .with_hook(Hook::BeforeUpdate(
            Phase::Validate,
            before_hook_sync_cx::<MockTableSource>(|cx| {
                let environment = cx
                    .record
                    .get("environment")
                    .or_else(|| cx.stored.and_then(|row| row.get("environment")));
                if environment == Some(&json!("dev")) {
                    return Err(vantage_core::error!("dev rows are frozen"));
                }
                Ok(())
            }),
        ));
    let err = table
        .patch_value("1".to_string(), &rec(json!({"name": "z"})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "dev rows are frozen");
}

/// The stored row is read once, through the set, and only when an update
/// hook wants it.
#[tokio::test]
async fn the_stored_row_is_read_once_and_only_for_update_hooks() {
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let counting = Hook::BeforeSelect(select_hook(move |_cx: SelectCx<'_, MockTableSource>| {
        count.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    let plain = table(vec![stored_row()]).await.with_hook(counting);
    plain
        .patch_value("1".to_string(), &rec(json!({"name": "b"})))
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    let hooked = plain
        .with_hook(seeing(Seen::default()))
        .with_hook(Hook::AfterUpdate(after_hook(
            |_cx: AfterCx<'_, MockTableSource>| Box::pin(async { Ok(()) }),
        )));
    hooked
        .patch_value("1".to_string(), &rec(json!({"name": "c"})))
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}
