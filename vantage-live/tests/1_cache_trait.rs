//! Test 1: Cache trait contract — get/put/invalidate_prefix.
//!
//! Each backend must satisfy: missing key returns None; put then get
//! returns the same data; invalidate_prefix drops all keys under the
//! prefix and only those keys.

use indexmap::IndexMap;
use vantage_live::cache::{Cache, CachedRows, MemCache, NoCache};
use vantage_types::Record;

fn empty_rows() -> CachedRows {
    CachedRows::new(IndexMap::new())
}

fn rows_with_one(name: &str) -> CachedRows {
    let mut r: Record<ciborium::Value> = Record::new();
    r.insert("name".into(), ciborium::Value::Text(name.into()));
    let mut map: IndexMap<String, Record<ciborium::Value>> = IndexMap::new();
    map.insert("id1".into(), r);
    CachedRows::new(map)
}

// ── MemCache ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn memcache_miss_returns_none() {
    let c = MemCache::new();
    assert!(c.get("absent").await.unwrap().is_none());
}

#[tokio::test]
async fn memcache_put_then_get_round_trip() {
    let c = MemCache::new();
    c.put("k", rows_with_one("Alice")).await.unwrap();

    let back = c.get("k").await.unwrap().expect("hit");
    assert_eq!(back.rows.len(), 1);
    assert_eq!(
        back.rows["id1"]["name"],
        ciborium::Value::Text("Alice".into())
    );
}

#[tokio::test]
async fn memcache_put_overwrites() {
    let c = MemCache::new();
    c.put("k", rows_with_one("Alice")).await.unwrap();
    c.put("k", rows_with_one("Bob")).await.unwrap();
    let back = c.get("k").await.unwrap().expect("hit");
    assert_eq!(
        back.rows["id1"]["name"],
        ciborium::Value::Text("Bob".into())
    );
}

#[tokio::test]
async fn memcache_invalidate_prefix_drops_matching() {
    let c = MemCache::new();
    c.put("clients/page_0", empty_rows()).await.unwrap();
    c.put("clients/page_1", empty_rows()).await.unwrap();
    c.put("orders/page_0", empty_rows()).await.unwrap();

    c.invalidate_prefix("clients").await.unwrap();

    assert!(c.get("clients/page_0").await.unwrap().is_none());
    assert!(c.get("clients/page_1").await.unwrap().is_none());
    assert!(c.get("orders/page_0").await.unwrap().is_some());
}

#[tokio::test]
async fn memcache_invalidate_prefix_empty_string_clears_all() {
    let c = MemCache::new();
    c.put("a", empty_rows()).await.unwrap();
    c.put("b", empty_rows()).await.unwrap();
    c.invalidate_prefix("").await.unwrap();
    assert!(c.get("a").await.unwrap().is_none());
    assert!(c.get("b").await.unwrap().is_none());
}

#[tokio::test]
async fn memcache_clone_shares_state() {
    let a = MemCache::new();
    let b = a.clone();
    a.put("k", rows_with_one("Alice")).await.unwrap();
    // The clone sees the same entry.
    assert!(b.get("k").await.unwrap().is_some());
}

// ── NoCache ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn nocache_get_always_returns_none() {
    let c = NoCache;
    assert!(c.get("anything").await.unwrap().is_none());
}

#[tokio::test]
async fn nocache_put_is_noop() {
    let c = NoCache;
    c.put("k", rows_with_one("Alice")).await.unwrap();
    // Still empty after.
    assert!(c.get("k").await.unwrap().is_none());
}

#[tokio::test]
async fn nocache_invalidate_prefix_succeeds() {
    let c = NoCache;
    c.invalidate_prefix("anything").await.unwrap();
}

// ── Trait-object usage ────────────────────────────────────────────────────

#[tokio::test]
async fn cache_can_be_used_as_dyn_trait() {
    use std::sync::Arc;
    let cache: Arc<dyn Cache> = Arc::new(MemCache::new());
    cache.put("k", rows_with_one("Alice")).await.unwrap();
    let v = cache.get("k").await.unwrap();
    assert!(v.is_some());
}
