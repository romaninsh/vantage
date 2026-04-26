//! Test 3: LiveTable write path — insert/replace/patch/delete go through
//! the queue, hit the master, and invalidate the cache.

mod common;

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_live::LiveTable;
use vantage_live::cache::{Cache, MemCache};
use vantage_types::Record;

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r: Record<CborValue> = Record::new();
    r.insert("name".into(), CborValue::Text(name.into()));
    r.insert("price".into(), CborValue::Integer(price.into()));
    r
}

#[tokio::test]
async fn insert_value_lands_on_master_and_invalidates_cache() {
    let (_tmp, master, typed) = common::seeded_redb_master("products").await;
    let cache = MemCache::new();
    let live = LiveTable::new(master, "products", Arc::new(cache.clone()));

    // Warm cache with a list_values, then insert.
    let _ = live.list_values().await.unwrap();
    assert!(cache.get(&live.page_cache_key(1)).await.unwrap().is_some());

    live.insert_value(&"d".to_string(), &record("Delta", 40))
        .await
        .unwrap();

    // Cache invalidated.
    assert!(cache.get(&live.page_cache_key(1)).await.unwrap().is_none());

    // Master sees the new row.
    use vantage_redb::Redb;
    use vantage_table::traits::table_source::TableSource;
    let typed_ds: &Redb = typed.data_source();
    let count = typed_ds.get_table_count(&typed).await.unwrap();
    assert_eq!(count, 4);
}

#[tokio::test]
async fn replace_value_updates_master() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let live = LiveTable::new(master, "products", Arc::new(MemCache::new()));

    live.replace_value(&"a".to_string(), &record("Alpha2", 99))
        .await
        .unwrap();

    let row = live
        .get_value(&"a".to_string())
        .await
        .unwrap()
        .expect("row exists");
    assert_eq!(row["name"], CborValue::Text("Alpha2".into()));
    assert_eq!(row["price"], CborValue::Integer(99i64.into()));
}

#[tokio::test]
async fn patch_value_merges_fields() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let live = LiveTable::new(master, "products", Arc::new(MemCache::new()));

    let mut partial: Record<CborValue> = Record::new();
    partial.insert("price".into(), CborValue::Integer(123i64.into()));

    live.patch_value(&"a".to_string(), &partial).await.unwrap();

    let row = live
        .get_value(&"a".to_string())
        .await
        .unwrap()
        .expect("row exists");
    // Patched field changed.
    assert_eq!(row["price"], CborValue::Integer(123i64.into()));
    // Untouched field stays.
    assert_eq!(row["name"], CborValue::Text("Alpha".into()));
}

#[tokio::test]
async fn delete_removes_from_master() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let live = LiveTable::new(master, "products", Arc::new(MemCache::new()));

    WritableValueSet::delete(&live, &"a".to_string())
        .await
        .unwrap();

    assert!(live.get_value(&"a".to_string()).await.unwrap().is_none());

    let remaining = live.list_values().await.unwrap();
    assert_eq!(remaining.len(), 2);
}

#[tokio::test]
async fn delete_all_drops_every_row() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let live = LiveTable::new(master, "products", Arc::new(MemCache::new()));

    WritableValueSet::delete_all(&live).await.unwrap();
    let remaining: IndexMap<_, _> = live.list_values().await.unwrap();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn cache_invalidated_for_id_slot_too() {
    // After warming both a page slot and an id slot, a single insert
    // should clear both because they share the cache_key prefix.
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let cache = MemCache::new();
    let live = LiveTable::new(master, "products", Arc::new(cache.clone()));

    let _ = live.list_values().await.unwrap();
    let _ = live.get_value(&"a".to_string()).await.unwrap();

    assert!(cache.get(&live.page_cache_key(1)).await.unwrap().is_some());
    assert!(cache.get(&live.id_cache_key("a")).await.unwrap().is_some());

    live.insert_value(&"e".to_string(), &record("Epsilon", 50))
        .await
        .unwrap();

    assert!(cache.get(&live.page_cache_key(1)).await.unwrap().is_none());
    assert!(cache.get(&live.id_cache_key("a")).await.unwrap().is_none());
}

#[tokio::test]
async fn failed_write_does_not_invalidate() {
    // Replace requires the row to exist in some backends; in redb's case
    // replace acts as upsert, so we instead test patch on a missing row
    // (which redb errors on).
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let cache = MemCache::new();
    let live = LiveTable::new(master, "products", Arc::new(cache.clone()));

    let _ = live.list_values().await.unwrap();
    let key = live.page_cache_key(1);
    assert!(cache.get(&key).await.unwrap().is_some());

    let mut partial: Record<CborValue> = Record::new();
    partial.insert("price".into(), CborValue::Integer(1i64.into()));
    let res = live.patch_value(&"missing".to_string(), &partial).await;
    assert!(res.is_err());

    // Cache still warm — no false invalidation.
    assert!(cache.get(&key).await.unwrap().is_some());
}
