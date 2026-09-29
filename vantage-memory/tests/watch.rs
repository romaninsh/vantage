use std::time::Duration;

use ciborium::Value as CborValue;
use futures_util::StreamExt;
use vantage_dataset::InsertableValueSet;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_vista::{VistaChange, VistaFactory};

const T: &str = "name: t\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  status: { type: string }\n";

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn status(s: &str) -> vantage_types::Record<CborValue> {
    [("status".to_string(), text(s))].into_iter().collect()
}

async fn next(s: &mut vantage_vista::VistaChangeStream) -> VistaChange {
    tokio::time::timeout(Duration::from_secs(2), s.next())
        .await
        .expect("change")
        .expect("open")
        .unwrap()
}

async fn nothing(s: &mut vantage_vista::VistaChangeStream) {
    assert!(
        tokio::time::timeout(Duration::from_millis(150), s.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn store_writes_reach_an_unfiltered_watch() {
    let store = MemoryStore::new();
    let v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let mut w = v.watch().await.unwrap();
    let t = store.table("t");
    t.upsert("a", status("Open"));
    assert!(matches!(next(&mut w).await, VistaChange::Inserted { id, .. } if id == "a"));
    t.patch("a", &status("Closed"));
    assert!(matches!(next(&mut w).await, VistaChange::Updated { id, .. } if id == "a"));
    t.delete("a");
    assert!(matches!(next(&mut w).await, VistaChange::Deleted { id } if id == "a"));
}

#[tokio::test]
async fn vista_writes_reach_the_watch() {
    let store = MemoryStore::new();
    let v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let mut w = v.watch().await.unwrap();
    v.insert_return_id_value(&status("Open")).await.unwrap();
    assert!(matches!(next(&mut w).await, VistaChange::Inserted { .. }));
}

#[tokio::test]
async fn row_entering_filter_is_inserted() {
    let store = MemoryStore::new();
    let mut v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    v.add_condition_eq("status", text("Open")).unwrap();
    let mut w = v.watch().await.unwrap();
    let t = store.table("t");
    t.upsert("a", status("Closed"));
    nothing(&mut w).await;
    t.patch("a", &status("Open"));
    assert!(matches!(next(&mut w).await, VistaChange::Inserted { id, .. } if id == "a"));
}

#[tokio::test]
async fn row_leaving_filter_is_deleted() {
    let store = MemoryStore::new();
    let t = store.table("t");
    t.upsert("a", status("Open"));
    let mut v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    v.add_condition_eq("status", text("Open")).unwrap();
    let mut w = v.watch().await.unwrap();
    t.patch("a", &status("Closed"));
    assert!(matches!(next(&mut w).await, VistaChange::Deleted { id } if id == "a"));
    t.delete("a");
    nothing(&mut w).await;
}

#[tokio::test]
async fn lag_becomes_invalidated() {
    let store = MemoryStore::new();
    let v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let mut w = v.watch().await.unwrap();
    let t = store.table("t");
    for i in 0..5000 {
        t.upsert(&i.to_string(), status("x"));
    }
    let mut saw_invalidated = false;
    while let Ok(Some(c)) = tokio::time::timeout(Duration::from_millis(300), w.next()).await {
        if matches!(c.unwrap(), VistaChange::Invalidated) {
            saw_invalidated = true;
            break;
        }
    }
    assert!(saw_invalidated);
}
