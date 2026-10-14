//! Opening on the cache, and following changes to the row.

use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Cache, CacheBackend, MemoryCache, RecordStatus};

use super::{build_lens, cbor_text, record, redb_dio, seeded_master, updated, wait_for_gen};

#[tokio::test]
async fn open_for_id_in_cache_returns_fresh() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").open().await?;
    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alpha")));
    assert_eq!(scenery.status(), RecordStatus::Fresh);
    Ok(())
}

#[tokio::test]
async fn a_miss_without_load_on_miss_is_not_found() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("missing").open().await?;
    assert!(scenery.record().is_none());
    assert_eq!(scenery.status(), RecordStatus::NotFound);
    Ok(())
}

#[tokio::test]
async fn an_applied_change_updates_record_and_bumps_generation() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    dio.apply_change(updated("a", Some(record("alphabet", 30))))
        .await?;
    wait_for_gen(&mut gen_rx, initial).await;

    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alphabet")));
    assert_eq!(scenery.status(), RecordStatus::Fresh);
    Ok(())
}

#[tokio::test]
async fn invalidate_other_id_does_not_bump() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    dio.apply_change(updated("b", None)).await?;

    // Give the bus a moment; no bump expected.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let after = u64::from(*gen_rx.borrow_and_update());
    assert_eq!(after, initial, "scenery for 'a' shouldn't react to 'b'");
    Ok(())
}

#[tokio::test]
async fn invalidate_own_id_triggers_reload() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    let lens = build_lens(Cache::backend(backend.clone()));
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.record_scenery("a").open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // Mutate the cache out-of-band, then publish a notify-only change.
    let table = backend.open_table("items").await?;
    table.insert_value("a", &record("alpha-prime", 31)).await?;
    dio.apply_change(updated("a", None)).await?;
    wait_for_gen(&mut gen_rx, initial).await;

    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alpha-prime")));
    Ok(())
}

#[tokio::test]
async fn a_seeded_scenery_skips_the_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    // `z` isn't cached: the row handed in is what the scenery shows.
    let handed = record("handoff", 99);
    let scenery = dio.record_scenery("z").seed(handed).open().await?;
    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("handoff")));
    assert_eq!(scenery.status(), RecordStatus::Fresh);
    Ok(())
}

#[tokio::test]
async fn scenery_outlives_dio_handle_drop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").open().await?;
    assert!(scenery.record().is_some());

    drop(dio);
    // Last-known state stays accessible; just no future reloads.
    assert!(scenery.record().is_some());
    Ok(())
}
