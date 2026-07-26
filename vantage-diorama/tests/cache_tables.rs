//! `CacheBackend::list_tables` / `drop_table` — enumerating and reclaiming the
//! named tables inside one backend.
//!
//! These are what let a consumer put every query variant of a datasource in a
//! single file (one table per variant) and still reclaim the ones that fall out
//! of use, rather than encoding the variant in a filename and orphaning files.

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{CacheBackend, MemoryCache, RedbCache};
use vantage_types::Record;

fn record(value: i64) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("v".to_string(), CborValue::Integer(value.into()));
    record
}

async fn exercise(backend: &dyn CacheBackend) -> Result<()> {
    let orders = backend.open_table("orders").await?;
    let clients = backend.open_table("clients").await?;
    orders.insert_value("a", &record(1)).await?;
    clients.insert_value("b", &record(2)).await?;

    let mut tables = backend.list_tables().await?;
    tables.sort();
    assert_eq!(tables, vec!["clients".to_string(), "orders".to_string()]);

    backend.drop_table("orders").await?;
    assert_eq!(backend.list_tables().await?, vec!["clients".to_string()]);

    // The surviving table is untouched.
    assert_eq!(clients.count().await?, 1);

    // Reopening a dropped name gives a fresh, empty table rather than the
    // rows that were there before.
    let reopened = backend.open_table("orders").await?;
    assert_eq!(reopened.count().await?, 0);

    // Dropping something absent is not an error.
    backend.drop_table("never_existed").await?;
    Ok(())
}

#[tokio::test]
async fn redb_lists_and_drops_tables() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = RedbCache::open(tmp.path().join("cache.redb"))?;
    exercise(&backend).await
}

#[tokio::test]
async fn memory_lists_and_drops_tables() -> Result<()> {
    exercise(&MemoryCache::new()).await
}

/// Every Dio under one Lens can share a single file, distinguished only by
/// table name — the shape that lets a datasource keep one cache file for all
/// of its query variants.
#[tokio::test]
async fn one_file_holds_many_independent_tables() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = RedbCache::open(tmp.path().join("datasource.redb"))?;

    for variant in ["orders", "orders-where-client=5", "orders?region=eu"] {
        let table = backend.open_table(variant).await?;
        table.insert_value("row", &record(1)).await?;
    }

    assert_eq!(backend.list_tables().await?.len(), 3);

    // Each variant's rows are its own.
    let narrowed = backend.open_table("orders-where-client=5").await?;
    narrowed.insert_value("extra", &record(2)).await?;
    assert_eq!(narrowed.count().await?, 2);
    assert_eq!(backend.open_table("orders").await?.count().await?, 1);
    Ok(())
}

/// A table opened but never written to does not exist yet — the handle is
/// lazy. Worth pinning: a sweep that expects to see every opened name would
/// otherwise be surprised.
#[tokio::test]
async fn an_unwritten_table_is_not_listed() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = RedbCache::open(tmp.path().join("cache.redb"))?;
    let _handle = backend.open_table("pending").await?;
    assert!(backend.list_tables().await?.is_empty());
    Ok(())
}
