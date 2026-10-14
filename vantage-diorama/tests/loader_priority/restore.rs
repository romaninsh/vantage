//! A cancelled load puts the visible map back as it found it: rows it never
//! bound are left alone, a row it overwrote goes back to the cached record,
//! and the restored slot is found by its id again.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::Lens;
use vantage_types::Record;

use super::fixtures::{
    Backend, Gated, GatedScenery, rec, release_the_open_fetch, rows, supersede_with_20_30,
    wait_until,
};
use crate::support::chunk::{col_at, master_with_id_type};

#[tokio::test]
async fn a_cancelled_refresh_over_unchanged_rows_keeps_them() -> Result<()> {
    let gated = Gated::new();
    let open = GatedScenery::open(&gated, rows(40)).await?;
    let scenery = &open.scenery;
    assert_eq!(col_at(scenery, 0, "v").as_deref(), Some("v0"));

    // A force_load refresh of the block just loaded: call 2 re-fetches
    // 0..10. Its early push re-sends the first rows unchanged — the
    // identical-fresh-record dedup in `write_chunk_row` answers `Skipped`
    // for them (they were already bound, unchanged, by call 1) — then it
    // blocks on the gate.
    scenery.refresh_source();
    wait_until("the refresh to start", || gated.calls_len() >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 0..10);

    // Supersede it with a different range before it finishes: call 2 is
    // cancelled. Its early-pushed rows were never *bound*, so the undo must
    // not touch them.
    scenery.set_viewport(20..30, 0..0);
    wait_until("the superseding load to start", || gated.calls_len() >= 3).await;
    assert_eq!(gated.calls.lock().unwrap()[2], 20..30);

    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;

    // Rows 0, 1 and 2 must still be present with their original values — a
    // cancelled load that only ever touched them through the dedup skip
    // must not have deleted them.
    assert_eq!(col_at(scenery, 0, "v").as_deref(), Some("v0"));
    assert_eq!(col_at(scenery, 1, "v").as_deref(), Some("v1"));
    assert_eq!(col_at(scenery, 2, "v").as_deref(), Some("v2"));
    Ok(())
}

/// The case the undo is really about: the cancelled load had already
/// overwritten a row with a *newer* value. Deleting that slot would punch a
/// hole in the grid where a perfectly good cached row was; the slot goes back
/// to the record it held instead.
#[tokio::test]
async fn a_cancelled_refresh_over_a_changed_row_restores_the_old_one() -> Result<()> {
    let gated = Gated::new();
    let backend = rows(40);
    let open = GatedScenery::open(&gated, backend.clone()).await?;
    let scenery = &open.scenery;
    assert_eq!(col_at(scenery, 3, "v").as_deref(), Some("v3"));

    // The backend moves under us, then a refresh picks the new value up in
    // its early push — row 3 is genuinely re-bound, with the old record
    // handed back as what it replaced.
    backend.lock().unwrap()[3].1 = rec("v3-new");
    scenery.refresh_source();
    wait_until("the refresh to start", || gated.calls_len() >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 0..10);
    wait_until("row 3 to take the new value", || {
        col_at(scenery, 3, "v").as_deref() == Some("v3-new")
    })
    .await;

    // Supersede the refresh before it can commit: the new value was never
    // written to the cache, so the slot must go back to the old record
    // rather than disappear.
    supersede_with_20_30(&gated, scenery).await;

    assert_eq!(
        col_at(scenery, 3, "v").as_deref(),
        Some("v3"),
        "the cancelled load's overwrite was rolled back to the cached record",
    );
    assert_eq!(col_at(scenery, 0, "v").as_deref(), Some("v0"));
    assert_eq!(col_at(scenery, 1, "v").as_deref(), Some("v1"));
    assert_eq!(col_at(scenery, 2, "v").as_deref(), Some("v2"));
    Ok(())
}

/// Rows whose id column is an integer, as a numeric-primary-key source
/// serves them. The cache key is still the string form, which is what
/// `cbor_id_to_string` is for.
fn rows_with_int_ids(n: usize) -> Backend {
    Arc::new(Mutex::new(
        (0..n)
            .map(|i| {
                let mut r = Record::new();
                r.insert("id".to_string(), CborValue::Integer((i as i64).into()));
                r.insert("v".to_string(), CborValue::Text(format!("v{i}")));
                (i.to_string(), r)
            })
            .collect(),
    ))
}

/// Restoring a slot re-registers its id, and "its id" is whatever the record
/// carries — an integer as readily as a string. Read back through the one
/// id-keyed path a test can drive from outside: a failed flash emits
/// `WriteReverted`, which the reactor resolves through `id_to_idx` and stamps
/// onto the row. No mapping, no stamp.
#[tokio::test]
async fn restore_re_registers_numeric_ids() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let backend = rows_with_int_ids(40);
    // A write route that always fails: a failing flash ends in
    // `WriteReverted`, which the reactor turns into a `mark_row` — the one
    // id-keyed operation a test can reach through the public surface, and
    // therefore how `id_to_idx` is observed from outside.
    let rejecting = Lens::builder()
        .writer(|_cx, _flash| async move { Err(vantage_core::error!("write route fails")) });
    let lens = gated.lens_from(rejecting, tmp.path().join("c.redb"), backend.clone());
    let dio = lens
        .dio(master_with_id_type("Int", &[("v", "String")]))
        .build()
        .await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;

    // Row 2 changes, so the refresh's early push re-binds exactly that slot
    // — and only it: the other early rows dedup away unchanged.
    let mut changed = Record::new();
    changed.insert("id".to_string(), CborValue::Integer(2.into()));
    changed.insert("v".to_string(), CborValue::Text("v2-new".into()));
    backend.lock().unwrap()[2].1 = changed;
    scenery.refresh_source();
    wait_until("the refresh to start", || gated.calls_len() >= 2).await;
    wait_until("row 2 to take the new value", || {
        col_at(&scenery, 2, "v").as_deref() == Some("v2-new")
    })
    .await;

    // Cancel it: row 2 is restored, and with it the mapping from its id.
    supersede_with_20_30(&gated, &scenery).await;
    assert_eq!(col_at(&scenery, 2, "v").as_deref(), Some("v2"));

    // The write route always fails, so this settles as `WriteFailed` on the
    // row the id resolves to — which requires the restored mapping.
    let mut patch = Record::new();
    patch.insert("v".to_string(), CborValue::Text("flashed".into()));
    let flash = vantage_diorama::ChangeFlash::partial("2", patch);
    assert!(dio.flash(flash).await.is_err());
    wait_until("row 2 to be stamped through its id", || {
        scenery.status_summary().failed == 1
    })
    .await;
    assert!(
        matches!(
            scenery.row(2).map(|r| r.status.clone()),
            Some(vantage_diorama::RowStatus::WriteFailed { .. })
        ),
        "the failed write landed on row 2, so its numeric id still maps to it",
    );
    Ok(())
}
