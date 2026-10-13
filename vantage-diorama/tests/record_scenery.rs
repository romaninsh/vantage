//! RecordScenery — single-row reactive view.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use tokio::sync::Notify;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{
    Cache, CacheBackend, ChangeEvent, ChangeFlash, Dio, FlashOutcome, Lens, Loader, MemoryCache,
    RecordStatus, RowStatus,
};
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

fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 30))
        .with_record("b", record("beta", 10));
    Vista::new("items", shell)
}

/// An eager loader copying the whole master into the cache.
fn copy_master() -> Loader {
    Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    })
}

fn build_lens(cache: Cache) -> Arc<Lens> {
    Lens::builder()
        .cache(cache)
        .loader(copy_master())
        .build()
        .expect("build lens")
}

async fn redb_dio(tmp: &TempDir) -> Result<Dio> {
    let lens = build_lens(Cache::redb(tmp.path().join("cache.redb")));
    lens.dio(seeded_master()).build().await
}

/// An update to `id`: carrying the row, or notify-only with `None`.
fn updated(id: &str, new: Option<Record<CborValue>>) -> ChangeEvent {
    ChangeEvent::Updated {
        id: id.to_string(),
        new,
    }
}

fn partial_name(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r
}

async fn wait_for_gen(
    rx: &mut tokio::sync::watch::Receiver<vantage_diorama::Generation>,
    current: u64,
) -> u64 {
    // Bounds a hang, not a speed — see `support/chunk.rs::wait_for_gen`.
    tokio::time::timeout(Duration::from_secs(10), async {
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

fn matches_status(status: &RecordStatus, expected: &RecordStatus) -> bool {
    matches!(
        (status, expected),
        (RecordStatus::Fresh, RecordStatus::Fresh)
            | (RecordStatus::Stale, RecordStatus::Stale)
            | (RecordStatus::Loading, RecordStatus::Loading)
            | (RecordStatus::NotFound, RecordStatus::NotFound)
            | (RecordStatus::Error(_), RecordStatus::Error(_))
    )
}

#[tokio::test]
async fn open_for_id_in_cache_returns_fresh() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").await?;
    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alpha")));
    assert!(matches_status(&scenery.status(), &RecordStatus::Fresh));
    Ok(())
}

#[tokio::test]
async fn open_for_missing_id_returns_not_found() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("missing").await?;
    assert!(scenery.record().is_none());
    assert!(matches_status(&scenery.status(), &RecordStatus::NotFound));
    Ok(())
}

#[tokio::test]
async fn an_applied_change_updates_record_and_bumps_generation() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    dio.apply_change(updated("a", Some(record("alphabet", 30))))
        .await?;
    wait_for_gen(&mut gen_rx, initial).await;

    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alphabet")));
    assert!(matches_status(&scenery.status(), &RecordStatus::Fresh));
    Ok(())
}

#[tokio::test]
async fn invalidate_other_id_does_not_bump() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").await?;
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

    let scenery = dio.record_scenery("a").await?;
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
async fn record_scenery_with_skips_cache_lookup() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    // No `await` — handed off synchronously with the row in hand.
    let handed = record("handoff", 99);
    let scenery = dio.record_scenery_with("z", handed.clone());
    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("handoff")));
    assert!(matches_status(&scenery.status(), &RecordStatus::Fresh));
    Ok(())
}

// ---- optimistic writes ------------------------------------------------------

/// A Dio whose Lens writer blocks on `gate` until released, so a test can
/// observe the `PendingWrite` window before the write-through completes.
async fn gated_write_dio(tmp: &TempDir, gate: Arc<Notify>) -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .loader(copy_master())
        .writer(move |_cx, _flash| {
            let gate = gate.clone();
            async move {
                gate.notified().await;
                Ok(FlashOutcome::applied())
            }
        })
        .build()?;
    lens.dio(seeded_master()).build().await
}

/// A Dio whose Lens writer always errors — drives the rollback path.
async fn erroring_write_dio(tmp: &TempDir) -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .loader(copy_master())
        .writer(|_cx, _flash| async move {
            Err::<FlashOutcome, _>(vantage_core::error!("upstream rejected"))
        })
        .build()?;
    lens.dio(seeded_master()).build().await
}

/// An optimistic write shows the new value as `PendingWrite` immediately, then
/// settles to `Fresh` once the write-through confirms.
#[tokio::test]
async fn optimistic_patch_shows_pending_then_fresh() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gate = Arc::new(Notify::new());
    let dio = gated_write_dio(&tmp, gate.clone()).await?;

    let scenery = dio.record_scenery("a").await?;
    let mut gen_rx = scenery.subscribe();
    let g0 = u64::from(*gen_rx.borrow_and_update());

    // Run the write on a task — it stages the value + emits WritePending, then
    // blocks in the writer until we release the gate.
    let dio2 = dio.clone();
    let handle = tokio::spawn(async move {
        dio2.flash(ChangeFlash::partial("a", partial_name("edited")))
            .await
    });

    wait_for_gen(&mut gen_rx, g0).await;
    let r = scenery.record().expect("record present");
    assert_eq!(
        r.record.get("name"),
        Some(&cbor_text("edited")),
        "value staged"
    );
    assert_eq!(
        r.record.get("price"),
        Some(&CborValue::Integer(30.into())),
        "patch merges"
    );
    assert!(
        matches!(r.status, RowStatus::PendingWrite),
        "got {:?}",
        r.status
    );

    // Release the write-through → confirms → Fresh.
    let g1 = u64::from(*gen_rx.borrow_and_update());
    gate.notify_one();
    handle.await.unwrap().expect("write commits");
    wait_for_gen(&mut gen_rx, g1).await;

    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("edited")));
    assert!(matches!(r.status, RowStatus::Fresh), "got {:?}", r.status);
    Ok(())
}

/// A failed optimistic write rolls the value back to the pre-image and flags the
/// row `WriteFailed`.
#[tokio::test]
async fn optimistic_patch_rolls_back_to_write_failed() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = erroring_write_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").await?;

    let outcome = dio
        .flash(ChangeFlash::partial("a", partial_name("edited")))
        .await;
    assert!(
        outcome.is_err(),
        "a rejecting write-through must surface the error"
    );

    // The reactor processes WritePending then WriteReverted; wait for the row to
    // settle on the failure.
    let mut failed = false;
    for _ in 0..200 {
        if let Some(r) = scenery.record()
            && matches!(r.status, RowStatus::WriteFailed { .. })
        {
            assert_eq!(
                r.record.get("name"),
                Some(&cbor_text("alpha")),
                "value reverted to the pre-image"
            );
            failed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(failed, "row never reached WriteFailed");
    Ok(())
}

/// An edit reflects across every open view of the record — two independent
/// `RecordScenery`s for the same id both update from the one cache row.
#[tokio::test]
async fn optimistic_edit_reflects_in_a_second_record_scenery() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let s1 = dio.record_scenery("a").await?;
    let s2 = dio.record_scenery("a").await?;

    // No writer → default write-through to master; commits.
    dio.flash(ChangeFlash::partial("a", partial_name("shared")))
        .await?;

    for (label, s) in [("s1", &s1), ("s2", &s2)] {
        let mut seen = false;
        for _ in 0..200 {
            if let Some(r) = s.record()
                && r.record.get("name") == Some(&cbor_text("shared"))
            {
                seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(seen, "{label} never observed the shared edit");
    }
    Ok(())
}

#[tokio::test]
async fn scenery_outlives_dio_handle_drop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp).await?;

    let scenery = dio.record_scenery("a").await?;
    assert!(scenery.record().is_some());

    drop(dio);
    // Last-known state stays accessible; just no future reloads.
    assert!(scenery.record().is_some());
    Ok(())
}
