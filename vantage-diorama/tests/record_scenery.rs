//! Stage 6: RecordScenery — single-row reactive view.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, RecordStatus};
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
    Vista::new("items", Box::new(shell))
}

async fn build_lens(cache_path: std::path::PathBuf) -> Result<Arc<Lens>> {
    Ok(Arc::new(
        Lens::new()
            .cache_at(cache_path)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    ))
}

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
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("a").await?;
    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alpha")));
    assert!(matches_status(&scenery.status(), &RecordStatus::Fresh));
    Ok(())
}

#[tokio::test]
async fn open_for_missing_id_returns_not_found() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("missing").await?;
    assert!(scenery.record().is_none());
    assert!(matches_status(&scenery.status(), &RecordStatus::NotFound));
    Ok(())
}

#[tokio::test]
async fn patched_updates_record_and_bumps_generation() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("a").await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    dio.patched("a", record("alphabet", 30)).await?;
    wait_for_gen(&mut gen_rx, initial).await;

    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alphabet")));
    assert!(matches_status(&scenery.status(), &RecordStatus::Fresh));
    Ok(())
}

#[tokio::test]
async fn invalidate_other_id_does_not_bump() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("a").await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    dio.notify_record_changed("b");

    // Give the bus a moment; no bump expected.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let after = u64::from(*gen_rx.borrow_and_update());
    assert_eq!(after, initial, "scenery for 'a' shouldn't react to 'b'");
    Ok(())
}

#[tokio::test]
async fn invalidate_own_id_triggers_reload() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("a").await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // Mutate the cache out-of-band, then publish without writing through `patched`.
    dio.cache()
        .insert_value("a", &record("alpha-prime", 31))
        .await?;
    dio.notify_record_changed("a");
    wait_for_gen(&mut gen_rx, initial).await;

    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("alpha-prime")));
    Ok(())
}

#[tokio::test]
async fn record_scenery_with_skips_cache_lookup() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    // No `await` — handed off synchronously with the row in hand.
    let handed = record("handoff", 99);
    let scenery = dio.record_scenery_with("z", handed.clone());
    let r = scenery.record().expect("record present");
    assert_eq!(r.record.get("name"), Some(&cbor_text("handoff")));
    assert!(matches_status(&scenery.status(), &RecordStatus::Fresh));
    Ok(())
}

// ---- optimistic writes ------------------------------------------------------

use tokio::sync::Notify;
use vantage_diorama::RowStatus;

fn partial_name(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r
}

/// Lens whose `on_write` blocks on `gate` until released, so a test can observe
/// the `PendingWrite` window before the write-through completes.
async fn build_lens_gated_write(cache_path: std::path::PathBuf, gate: Arc<Notify>) -> Arc<Lens> {
    Arc::new(
        Lens::new()
            .cache_at(cache_path)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .on_write(move |_dio, _op| {
                let gate = gate.clone();
                async move {
                    gate.notified().await;
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    )
}

/// Lens whose `on_write` always errors — drives the rollback path.
async fn build_lens_erroring_write(cache_path: std::path::PathBuf) -> Arc<Lens> {
    Arc::new(
        Lens::new()
            .cache_at(cache_path)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .on_write(|_dio, _op| async move { Err(vantage_core::error!("upstream rejected")) })
            .build()
            .expect("build lens"),
    )
}

/// An optimistic write shows the new value as `PendingWrite` immediately, then
/// settles to `Fresh` once the write-through confirms.
#[tokio::test]
async fn optimistic_patch_shows_pending_then_fresh() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gate = Arc::new(Notify::new());
    let lens = build_lens_gated_write(tmp.path().join("cache.redb"), gate.clone()).await;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("a").await?;
    let mut gen_rx = scenery.subscribe();
    let g0 = u64::from(*gen_rx.borrow_and_update());

    // Run the write on a task — it stages the value + emits WritePending, then
    // blocks in on_write until we release the gate.
    let dio2 = dio.clone();
    let handle =
        tokio::spawn(async move { dio2.patch_optimistic("a", partial_name("edited")).await });

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
    let lens = build_lens_erroring_write(tmp.path().join("cache.redb")).await;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("a").await?;

    let outcome = dio.patch_optimistic("a", partial_name("edited")).await;
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
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let s1 = dio.record_scenery("a").await?;
    let s2 = dio.record_scenery("a").await?;

    // No on_write → default write-through to master; commits.
    dio.patch_optimistic("a", partial_name("shared")).await?;

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
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.make_dio(seeded_master()).await?;

    let scenery = dio.record_scenery("a").await?;
    assert!(scenery.record().is_some());

    drop(dio);
    // Last-known state stays accessible; just no future reloads.
    assert!(scenery.record().is_some());
    Ok(())
}
