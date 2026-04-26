//! Test 2: LiveTable read path — cache miss → master fetch → cache
//! populated → next read is a hit.
//!
//! Master is a tempfile-backed redb (see `tests/common/`). Cache is
//! `MemCache`. Each test seeds the master, wraps it as a LiveTable, and
//! exercises one specific read scenario.

mod common;

use std::sync::Arc;

use vantage_dataset::traits::ReadableValueSet;
use vantage_live::LiveTable;
use vantage_live::cache::{Cache, MemCache};

type Mem = MemCache;

#[tokio::test]
async fn list_values_cache_miss_then_hit() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;

    let cache = Mem::new();
    let live = LiveTable::new(master, "products", Arc::new(cache.clone()));

    // First read: miss → master fetch → cache populated.
    let first = live.list_values().await.unwrap();
    assert_eq!(first.len(), 3);

    // Cache should now have an entry under `products/page_1`.
    let key = live.page_cache_key(1);
    assert!(cache.get(&key).await.unwrap().is_some());

    // Second read: hit → cache returns the same rows.
    let second = live.list_values().await.unwrap();
    assert_eq!(second.len(), 3);
    assert_eq!(
        first.keys().collect::<Vec<_>>(),
        second.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn list_values_with_nocache_passes_through() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let live = LiveTable::new(master, "products", Arc::new(vantage_live::cache::NoCache));

    let rows = live.list_values().await.unwrap();
    assert_eq!(rows.len(), 3);
    // No cache, so a second read also goes to master — assert idempotency.
    let rows2 = live.list_values().await.unwrap();
    assert_eq!(rows2.len(), 3);
}

#[tokio::test]
async fn get_value_caches_per_id() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let cache = Mem::new();
    let live = LiveTable::new(master, "products", Arc::new(cache.clone()));

    let row = live.get_value(&"a".to_string()).await.unwrap();
    assert!(row.is_some());

    // Per-id cache slot is distinct from page slots.
    let id_key = live.id_cache_key("a");
    let page_key = live.page_cache_key(1);
    assert!(cache.get(&id_key).await.unwrap().is_some());
    assert!(cache.get(&page_key).await.unwrap().is_none());
}

#[tokio::test]
async fn get_value_missing_id_returns_none_and_doesnt_cache() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let cache = Mem::new();
    let live = LiveTable::new(master, "products", Arc::new(cache.clone()));

    let result = live.get_value(&"absent".to_string()).await.unwrap();
    assert!(result.is_none());

    // Negative results aren't cached — next call still goes to master.
    assert!(
        cache
            .get(&live.id_cache_key("absent"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn pagination_distinct_pages_cached_separately() {
    use vantage_table::pagination::Pagination;

    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let cache = Mem::new();
    let mut live = LiveTable::new(master, "products", Arc::new(cache.clone()));

    // Page 1 of 2-per-page.
    {
        use vantage_table::traits::table_like::TableLike;
        live.set_pagination(Some(Pagination::new(1, 2)));
    }
    let p1 = live.list_values().await.unwrap();

    // Page 2.
    {
        use vantage_table::traits::table_like::TableLike;
        live.set_pagination(Some(Pagination::new(2, 2)));
    }
    let p2 = live.list_values().await.unwrap();

    // They cache to different keys.
    assert!(cache.get(&live.page_cache_key(1)).await.unwrap().is_some());
    assert!(cache.get(&live.page_cache_key(2)).await.unwrap().is_some());

    // Combined size matches the master total.
    assert_eq!(p1.len() + p2.len(), 3);
}
