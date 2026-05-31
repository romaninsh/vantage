//! Stage 5: TableScenery — reactive ordered-rows view.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, SortDir};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

/// Build a MockShell-backed master pre-seeded with three rows.
fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 30))
        .with_record("b", record("beta", 10))
        .with_record("c", record("gamma", 20));
    Vista::new("items", Box::new(shell))
}

/// Build a Lens whose `on_start` copies the master into the cache so
/// the Scenery has something to read.
async fn build_lens(cache_path: std::path::PathBuf) -> Result<Arc<Lens>> {
    let lens = Lens::new()
        .cache_at(cache_path)
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await
            }
        })
        .build()
        .expect("build lens");
    Ok(Arc::new(lens))
}

/// Wait for the Scenery's generation to advance past `current`.
async fn wait_for_gen(
    rx: &mut tokio::sync::watch::Receiver<vantage_diorama::Generation>,
    current: u64,
) -> u64 {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > current {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.expect("watch channel closed");
        }
    })
    .await
    .expect("timed out waiting for generation bump")
}

#[tokio::test]
async fn scenery_loads_rows_from_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.table_scenery().open().await?;
    assert_eq!(scenery.row_count(), 3);
    assert!(scenery.row(0).is_some());
    assert!(scenery.row(2).is_some());
    assert!(scenery.row(3).is_none());
    Ok(())
}

#[tokio::test]
async fn set_sort_reorders_and_bumps_generation() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    scenery.set_sort(Some("price".to_string()), SortDir::Asc);
    wait_for_gen(&mut gen_rx, initial).await;

    let r0 = scenery.row(0).unwrap();
    let r2 = scenery.row(2).unwrap();
    assert_eq!(r0.record.get("name"), Some(&cbor_text("beta"))); // price 10
    assert_eq!(r2.record.get("name"), Some(&cbor_text("alpha"))); // price 30
    Ok(())
}

#[tokio::test]
async fn set_search_filters() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    scenery.set_search(Some("alph".to_string()));
    wait_for_gen(&mut gen_rx, initial).await;
    assert_eq!(scenery.row_count(), 1);
    let only = scenery.row(0).unwrap();
    assert_eq!(only.record.get("name"), Some(&cbor_text("alpha")));

    // Clearing search restores all rows.
    let after = u64::from(*gen_rx.borrow_and_update());
    scenery.set_search(None);
    wait_for_gen(&mut gen_rx, after).await;
    assert_eq!(scenery.row_count(), 3);
    Ok(())
}

#[tokio::test]
async fn where_eq_at_builder_time_filters_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio
        .table_scenery()
        .where_eq("name", cbor_text("gamma"))
        .open()
        .await?;
    assert_eq!(scenery.row_count(), 1);
    assert_eq!(
        scenery.row(0).unwrap().record.get("price"),
        Some(&CborValue::Integer(20.into()))
    );
    Ok(())
}

#[tokio::test]
async fn external_invalidate_record_triggers_reload() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // External system tells the Dio about a new row, then publishes a change.
    dio.cache().insert_value("d", &record("delta", 5)).await?;
    dio.invalidate_record("d");

    wait_for_gen(&mut gen_rx, initial).await;
    assert_eq!(scenery.row_count(), 4);
    Ok(())
}

#[tokio::test]
async fn patched_updates_visible_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // Live stream renamed "alpha" → "alphabet".
    dio.patched("a", record("alphabet", 30)).await?;
    wait_for_gen(&mut gen_rx, initial).await;

    let updated = scenery
        .row(0)
        .or_else(|| scenery.row(1))
        .or_else(|| scenery.row(2))
        .expect("row present");
    // Find the renamed row; order may have shifted if sort were set.
    let mut found = None;
    for i in 0..scenery.row_count() {
        let r = scenery.row(i).unwrap();
        if r.record.get("name") == Some(&cbor_text("alphabet")) {
            found = Some(r);
            break;
        }
    }
    let r = found.expect("found renamed row");
    let _ = updated;
    assert_eq!(r.record.get("name"), Some(&cbor_text("alphabet")));
    Ok(())
}

/// `dio.removed(id)` wipes the cache entry AND publishes the bus
/// event so a subscribed TableScenery's reseed actually drops the
/// row. The bare `invalidate_record(id)` path leaves the cache
/// untouched — sceneries would reload the deleted row right back in.
#[tokio::test]
async fn removed_drops_row_from_visible_set() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());
    assert_eq!(scenery.row_count(), 3);

    dio.removed("b").await?;
    wait_for_gen(&mut gen_rx, initial).await;

    assert_eq!(scenery.row_count(), 2);
    let names: Vec<_> = (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|r| r.record.get("name").cloned())
        .collect();
    assert!(names.contains(&cbor_text("alpha")));
    assert!(!names.contains(&cbor_text("beta")));
    Ok(())
}

/// `dio.removed(id)` is idempotent — calling on an absent id is a
/// no-op (still publishes the event so any reseed-style scenery
/// re-syncs harmlessly).
#[tokio::test]
async fn removed_is_idempotent_on_missing_id() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let _scenery = dio.table_scenery().open().await?;
    dio.removed("does-not-exist").await?;
    Ok(())
}

#[tokio::test]
async fn scenery_outlives_dio_handle_drop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.table_scenery().open().await?;
    assert_eq!(scenery.row_count(), 3);

    // Drop the external Dio handle. Scenery still has its rows in
    // memory (just no future reloads).
    drop(dio);
    assert_eq!(scenery.row_count(), 3);
    Ok(())
}
