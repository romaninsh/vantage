//! Applied changes reaching an open view: notify-only updates, carried
//! rows, and deletes.

use std::sync::Arc;

use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Cache, CacheBackend, ChangeEvent, Lens, RedbCache};

use super::{build_lens, cbor_text, record, seeded_master, wait_for_gen};
use crate::support::copy_master;

#[tokio::test]
async fn external_notify_record_changed_triggers_reload() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = Arc::new(RedbCache::open(tmp.path().join("cache.redb"))?);
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(copy_master())
        .build()?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // External system writes a new row into the cache, then publishes a
    // notify-only change.
    let table = backend.open_table("items").await?;
    table.insert_value("d", &record("delta", 5)).await?;
    dio.apply_change(ChangeEvent::Updated {
        id: "d".into(),
        new: None,
    })
    .await?;

    wait_for_gen(&mut gen_rx, initial).await;
    assert_eq!(scenery.row_count(), 4);
    Ok(())
}

#[tokio::test]
async fn patched_updates_visible_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // Live stream renamed "alpha" → "alphabet".
    dio.apply_change(ChangeEvent::Updated {
        id: "a".into(),
        new: Some(record("alphabet", 30)),
    })
    .await?;
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

/// An applied `Deleted` change wipes the cache entry AND publishes the bus
/// event so a subscribed TableScenery's reseed actually drops the row. A
/// notify-only change leaves the cache untouched — sceneries would reload
/// the deleted row right back in.
#[tokio::test]
async fn removed_drops_row_from_visible_set() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());
    assert_eq!(scenery.row_count(), 3);

    dio.apply_change(ChangeEvent::Deleted { id: "b".into() })
        .await?;
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

/// A `Deleted` change is idempotent — applying one for an absent id is a
/// no-op (still publishes the event so any reseed-style scenery re-syncs
/// harmlessly).
#[tokio::test]
async fn removed_is_idempotent_on_missing_id() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let _scenery = dio.table_scenery().open().await?;
    dio.apply_change(ChangeEvent::Deleted {
        id: "does-not-exist".into(),
    })
    .await?;
    Ok(())
}
