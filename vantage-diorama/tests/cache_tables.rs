//! `CacheBackend::list_tables` / `drop_table` — enumerating and reclaiming the
//! named tables inside one backend.
//!
//! These are what let a consumer put every query variant of a datasource in a
//! single file (one table per variant) and still reclaim the ones that fall out
//! of use, rather than encoding the variant in a filename and orphaning files.

mod support;

use std::time::SystemTime;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Cache, CacheBackend, Lens, MemoryCache, RedbCache};
use vantage_types::Record;
use vantage_vista::{Condition, FilterOp, SortDirection, VistaQuery};

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

/// Dropping a table takes its meta with it, and meta never shows up as a
/// table of its own.
async fn drop_forgets_meta(backend: &dyn CacheBackend) -> Result<()> {
    let orders = backend.open_table("orders").await?;
    orders.insert_value("a", &record(1)).await?;
    orders.set_meta_total(7).await?;
    orders.set_meta_refreshed_at(SystemTime::now()).await?;
    assert_eq!(backend.list_tables().await?, vec!["orders".to_string()]);

    backend.drop_table("orders").await?;
    assert!(backend.list_tables().await?.is_empty());

    let reopened = backend.open_table("orders").await?;
    assert_eq!(reopened.meta_total().await?, None);
    assert_eq!(reopened.meta_refreshed_at().await?, None);
    Ok(())
}

#[tokio::test]
async fn redb_drop_forgets_meta() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = RedbCache::open(tmp.path().join("cache.redb"))?;
    drop_forgets_meta(&backend).await
}

#[tokio::test]
async fn memory_drop_forgets_meta() -> Result<()> {
    drop_forgets_meta(&MemoryCache::new()).await
}

/// Clearing a table forgets when it was last refreshed and copied whole, so
/// an emptied cache never looks fresh to a refresher's `max_age` nor serves
/// a list pass; the stated total stays.
async fn clear_forgets_refresh_time(backend: &dyn CacheBackend) -> Result<()> {
    let orders = backend.open_table("orders").await?;
    orders.insert_value("a", &record(1)).await?;
    orders.set_meta_total(7).await?;
    orders.set_meta_refreshed_at(SystemTime::now()).await?;
    orders.set_meta_copied_at(SystemTime::now()).await?;

    orders.clear().await?;
    assert_eq!(orders.count().await?, 0);
    assert_eq!(orders.meta_refreshed_at().await?, None);
    assert_eq!(orders.meta_copied_at().await?, None);
    assert_eq!(orders.meta_total().await?, Some(7));
    Ok(())
}

#[tokio::test]
async fn redb_clear_forgets_refresh_time() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = RedbCache::open(tmp.path().join("cache.redb"))?;
    clear_forgets_refresh_time(&backend).await
}

#[tokio::test]
async fn memory_clear_forgets_refresh_time() -> Result<()> {
    clear_forgets_refresh_time(&MemoryCache::new()).await
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

/// A query whose key carries every kind of character `VistaQuery::key`
/// passes through: separators, quotes, slashes, spaces, non-ASCII text,
/// typed values and nested arrays.
fn awkward_query() -> VistaQuery {
    VistaQuery::new()
        .with_condition(Condition::new(
            "na:me/x",
            FilterOp::Eq,
            "a?b=c&d+e-f \"q\" 'r' \\ 日本 🦀",
        ))
        .with_condition(Condition::new("n", FilterOp::Gt, 3.5))
        .with_condition(Condition::new(
            "tags",
            FilterOp::InSet,
            CborValue::Array(vec![CborValue::Bool(true), CborValue::Null]),
        ))
        .with_search("x y")
        .with_order("na:me/x", SortDirection::Descending)
}

/// Both backends take a `name?key` table name as it is, for every printable
/// character a key can hold.
async fn key_names_round_trip(backend: &dyn CacheBackend) -> Result<()> {
    let name = format!("orders?{}", awkward_query().key());
    let table = backend.open_table(&name).await?;
    table.insert_value("a", &record(1)).await?;
    assert_eq!(backend.list_tables().await?, vec![name.clone()]);
    assert_eq!(backend.open_table(&name).await?.count().await?, 1);
    backend.drop_table(&name).await?;
    assert!(backend.list_tables().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn redb_takes_query_key_names() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    key_names_round_trip(&RedbCache::open(tmp.path().join("cache.redb"))?).await
}

#[tokio::test]
async fn memory_takes_query_key_names() -> Result<()> {
    key_names_round_trip(&MemoryCache::new()).await
}

/// Why a Dio encodes control characters: redb marks its meta tables with
/// one, so a raw name holding it vanishes from `list_tables`.
#[tokio::test]
async fn redb_hides_a_raw_control_character_name() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = RedbCache::open(tmp.path().join("cache.redb"))?;
    let table = backend.open_table("orders?red\u{1}meta").await?;
    table.insert_value("a", &record(1)).await?;
    assert!(backend.list_tables().await?.is_empty());
    Ok(())
}

/// A control character can reach a key through a value. A Dio's cache table
/// name encodes it, and stays listed.
#[tokio::test]
async fn a_dio_encodes_control_characters_in_its_table_name() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("cache.redb");
    let lens = Lens::builder()
        .cache(Cache::redb(&path))
        .loader(support::copy_master())
        .build()?;
    let dio = lens.dio(support::teams_master()).build().await?;
    let narrowed = dio
        .with_query(&support::cond("team", FilterOp::Eq, "red\u{1}meta"))
        .await?;
    narrowed
        .apply_change(vantage_diorama::ChangeEvent::Inserted {
            id: "z".into(),
            new: Some(record(1)),
        })
        .await?;
    drop((dio, narrowed, lens));

    // A narrowed Dio's table is the vista's name, `?`, then its query key.
    let tables = RedbCache::open(&path)?.list_tables().await?;
    let narrowed: Vec<&String> = tables.iter().filter(|t| t.contains('?')).collect();
    assert_eq!(
        narrowed.len(),
        1,
        "the narrowed table is listed: {tables:?}"
    );
    assert!(!narrowed[0].chars().any(char::is_control), "{tables:?}");
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
