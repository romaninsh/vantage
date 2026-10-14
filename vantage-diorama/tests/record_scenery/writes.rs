//! Optimistic writes as a record scenery shows them.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use tokio::sync::Notify;
use vantage_core::Result;
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashOutcome, Lens, RowStatus};

use super::{cbor_text, copy_master, partial_name, redb_dio, seeded_master, wait_for_gen};

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

    let scenery = dio.record_scenery("a").open().await?;
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

    let scenery = dio.record_scenery("a").open().await?;

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

    let s1 = dio.record_scenery("a").open().await?;
    let s2 = dio.record_scenery("a").open().await?;

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
