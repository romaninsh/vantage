//! Test 5: a `LiveTable` wrapped via `AnyTable::new` is a drop-in
//! replacement for any `AnyTable`-shaped consumer.

mod common;

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_live::LiveTable;
use vantage_live::cache::MemCache;
use vantage_table::any::AnyTable;
use vantage_types::Record;

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r: Record<CborValue> = Record::new();
    r.insert("name".into(), CborValue::Text(name.into()));
    r.insert("price".into(), CborValue::Integer(price.into()));
    r
}

#[tokio::test]
async fn live_table_wraps_into_any_table() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let live = LiveTable::new(master, "products", Arc::new(MemCache::new()));

    // Wrap.
    let any = AnyTable::from_table_like(live);

    // Reads work through the AnyTable-shaped trait surface.
    let rows = any.list_values().await.unwrap();
    assert_eq!(rows.len(), 3);

    // Writes too.
    any.insert_value(&"new".to_string(), &record("New", 50))
        .await
        .unwrap();
    let rows = any.list_values().await.unwrap();
    assert_eq!(rows.len(), 4);
}

#[tokio::test]
async fn anytable_metadata_passes_through_to_master() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let live = LiveTable::new(master, "products", Arc::new(MemCache::new()));
    let any = AnyTable::from_table_like(live);

    use vantage_table::traits::table_like::TableLike;
    assert_eq!(any.table_name(), "products");
    assert!(
        any.column_names()
            .iter()
            .any(|n| n == "name" || n == "price")
    );
}
