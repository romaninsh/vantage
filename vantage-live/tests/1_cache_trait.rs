//! Test 1: Cache trait contract — get/put/invalidate_prefix.
//!
//! Each backend must satisfy: missing key returns None; put then get
//! returns the same data; invalidate_prefix drops all keys under the
//! prefix and only those keys.

use indexmap::IndexMap;
use vantage_live::cache::{Cache, CachedRows, MemCache, NoCache, RedbCache};
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

// ── RedbCache ────────────────────────────────────────────────────────────
//
// Each test gets its own tempdir so they don't fight for the redb file
// lock. The RedbCache is dropped at end of scope; that releases the
// lock, which lets us re-open the same folder in the persistence test
// to verify state survived.

fn fresh_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[tokio::test]
async fn redbcache_miss_returns_none() {
    let dir = fresh_dir();
    let c = RedbCache::open(dir.path()).unwrap();
    assert!(c.get("clients/page_1").await.unwrap().is_none());
}

#[tokio::test]
async fn redbcache_put_then_get_round_trip() {
    let dir = fresh_dir();
    let c = RedbCache::open(dir.path()).unwrap();

    c.put("clients/page_1", rows_with_one("Alice"))
        .await
        .unwrap();
    let back = c.get("clients/page_1").await.unwrap().expect("hit");
    assert_eq!(back.rows.len(), 1);
    assert_eq!(
        back.rows["id1"]["name"],
        ciborium::Value::Text("Alice".into())
    );
}

#[tokio::test]
async fn redbcache_put_overwrites() {
    let dir = fresh_dir();
    let c = RedbCache::open(dir.path()).unwrap();

    c.put("k/page_1", rows_with_one("Alice")).await.unwrap();
    c.put("k/page_1", rows_with_one("Bob")).await.unwrap();
    let back = c.get("k/page_1").await.unwrap().expect("hit");
    assert_eq!(
        back.rows["id1"]["name"],
        ciborium::Value::Text("Bob".into())
    );
}

#[tokio::test]
async fn redbcache_invalidate_root_drops_all_subkeys() {
    let dir = fresh_dir();
    let c = RedbCache::open(dir.path()).unwrap();

    c.put("clients/page_1", rows_with_one("A")).await.unwrap();
    c.put("clients/page_2", rows_with_one("B")).await.unwrap();
    c.put("clients/id/marty", rows_with_one("M")).await.unwrap();
    c.put("orders/page_1", rows_with_one("O")).await.unwrap();

    // Drop the whole "clients" cache_key.
    c.invalidate_prefix("clients").await.unwrap();

    assert!(c.get("clients/page_1").await.unwrap().is_none());
    assert!(c.get("clients/page_2").await.unwrap().is_none());
    assert!(c.get("clients/id/marty").await.unwrap().is_none());
    // Other cache_keys untouched.
    assert!(c.get("orders/page_1").await.unwrap().is_some());
}

#[tokio::test]
async fn redbcache_invalidate_subprefix_inside_a_table() {
    // The fast path is "prefix == cache_key → drop table". The slow
    // path matches sub-prefixes within a table. Test the slow path too.
    let dir = fresh_dir();
    let c = RedbCache::open(dir.path()).unwrap();

    c.put("clients/page_1", rows_with_one("A")).await.unwrap();
    c.put("clients/page_2", rows_with_one("B")).await.unwrap();
    c.put("clients/id/marty", rows_with_one("M")).await.unwrap();

    // Sub-prefix: invalidate only the page_* entries, leave id/marty.
    c.invalidate_prefix("clients/page_").await.unwrap();

    assert!(c.get("clients/page_1").await.unwrap().is_none());
    assert!(c.get("clients/page_2").await.unwrap().is_none());
    assert!(c.get("clients/id/marty").await.unwrap().is_some());
}

#[tokio::test]
async fn redbcache_persists_across_handles() {
    // Drop the cache → release file lock → open it again at the same
    // path. State should still be there.
    let dir = fresh_dir();
    {
        let c = RedbCache::open(dir.path()).unwrap();
        c.put("clients/page_1", rows_with_one("Alice"))
            .await
            .unwrap();
        // c drops here, releasing the lock.
    }

    let c2 = RedbCache::open(dir.path()).unwrap();
    let back = c2
        .get("clients/page_1")
        .await
        .unwrap()
        .expect("data persisted");
    assert_eq!(
        back.rows["id1"]["name"],
        ciborium::Value::Text("Alice".into())
    );
}

#[tokio::test]
async fn redbcache_clone_shares_state() {
    let dir = fresh_dir();
    let a = RedbCache::open(dir.path()).unwrap();
    let b = a.clone();

    a.put("clients/page_1", rows_with_one("Alice"))
        .await
        .unwrap();
    assert!(b.get("clients/page_1").await.unwrap().is_some());
}

#[tokio::test]
async fn redbcache_creates_folder_if_missing() {
    // The point of taking a folder rather than a file path: missing
    // directory is created, no fuss.
    let parent = tempfile::tempdir().unwrap();
    let nested = parent.path().join("nested/cache");
    assert!(!nested.exists());
    let c = RedbCache::open(&nested).unwrap();
    assert!(nested.exists());

    // And it actually works.
    c.put("k/page_1", rows_with_one("Alice")).await.unwrap();
    assert!(c.get("k/page_1").await.unwrap().is_some());
}

#[tokio::test]
async fn redbcache_no_cross_root_collisions() {
    // Two cache_keys with similar names ("client" / "clients") must not
    // see each other. The namespace prefix should keep them apart.
    let dir = fresh_dir();
    let c = RedbCache::open(dir.path()).unwrap();

    c.put("client/page_1", rows_with_one("Singular"))
        .await
        .unwrap();
    c.put("clients/page_1", rows_with_one("Plural"))
        .await
        .unwrap();

    let s = c.get("client/page_1").await.unwrap().expect("hit");
    let p = c.get("clients/page_1").await.unwrap().expect("hit");
    assert_eq!(
        s.rows["id1"]["name"],
        ciborium::Value::Text("Singular".into())
    );
    assert_eq!(
        p.rows["id1"]["name"],
        ciborium::Value::Text("Plural".into())
    );

    // Invalidating "client" should not affect "clients".
    c.invalidate_prefix("client").await.unwrap();
    assert!(c.get("client/page_1").await.unwrap().is_none());
    assert!(c.get("clients/page_1").await.unwrap().is_some());
}
