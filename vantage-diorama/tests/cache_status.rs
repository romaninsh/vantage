//! The detail cache persists each record's completeness status so two-pass
//! hydration can resume across restarts and skip already-complete records,
//! and each table's meta (stated total, last refresh) beside its rows.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_diorama::{CacheBackend, CacheStatus, CacheTable, MemoryCache, RedbCache};
use vantage_types::Record;

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

#[tokio::test]
async fn status_persists_across_reopen() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("cache.redb");

    {
        let cache = RedbCache::open(&path).unwrap();
        let table = cache.open_table("items").await.unwrap();
        table
            .insert_value_with_status("a", &rec("a"), CacheStatus::Incomplete)
            .await
            .unwrap();
        // The status-agnostic write defaults to Complete.
        table.insert_value("b", &rec("b")).await.unwrap();

        assert_eq!(
            table.get_value_with_status("a").await.unwrap().unwrap().1,
            CacheStatus::Incomplete
        );
        assert_eq!(
            table.get_value_with_status("b").await.unwrap().unwrap().1,
            CacheStatus::Complete
        );
    }

    // Reopen the same file: the persisted status survives.
    let cache = RedbCache::open(&path).unwrap();
    let table = cache.open_table("items").await.unwrap();

    let (rec_a, status_a) = table.get_value_with_status("a").await.unwrap().unwrap();
    assert_eq!(status_a, CacheStatus::Incomplete);
    assert_eq!(rec_a.get("v"), Some(&CborValue::Text("a".to_string())));
    assert_eq!(
        table.get_value_with_status("b").await.unwrap().unwrap().1,
        CacheStatus::Complete
    );

    // The status-agnostic read still returns the record.
    assert!(table.get_value("a").await.unwrap().is_some());
}

async fn assert_meta_round_trips(table: &dyn CacheTable, at: SystemTime) {
    table.set_meta_total(42).await.unwrap();
    table.set_meta_refreshed_at(at).await.unwrap();
    assert_eq!(table.meta_total().await.unwrap(), Some(42));
    assert_eq!(table.meta_refreshed_at().await.unwrap(), Some(at));
}

#[tokio::test]
async fn meta_persists_across_reopen() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("cache.redb");
    let at = SystemTime::now();

    {
        let cache = RedbCache::open(&path).unwrap();
        let table = cache.open_table("items").await.unwrap();
        assert_eq!(table.meta_total().await.unwrap(), None);
        assert_eq!(table.meta_refreshed_at().await.unwrap(), None);
        assert_meta_round_trips(table.as_ref(), at).await;
    }

    let cache = RedbCache::open(&path).unwrap();
    let table = cache.open_table("items").await.unwrap();
    assert_eq!(table.meta_total().await.unwrap(), Some(42));
    assert_eq!(table.meta_refreshed_at().await.unwrap(), Some(at));
    // Meta lives beside the rows, never among them.
    assert_eq!(table.count().await.unwrap(), 0);

    // A time before the epoch can't be stored: an error, and the stored
    // time stands.
    let before_epoch = UNIX_EPOCH - Duration::from_secs(1);
    assert!(table.set_meta_refreshed_at(before_epoch).await.is_err());
    assert_eq!(table.meta_refreshed_at().await.unwrap(), Some(at));
}

#[tokio::test]
async fn memory_meta_round_trips() {
    let cache = MemoryCache::new();
    let table = cache.open_table("items").await.unwrap();
    assert_eq!(table.meta_refreshed_at().await.unwrap(), None);
    assert_meta_round_trips(table.as_ref(), SystemTime::now()).await;
}
