//! Refresh against hydrated rows: an unchanged master keeps its augments and
//! fetches nothing, and a changed row refetches only its own.

use std::time::Duration;

use tempfile::TempDir;
use vantage_dataset::prelude::WritableValueSet;
use vantage_diorama::Source;
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

use crate::support::{self, augmented_dio, fixed_augmentation, meta, poll_until, record};
use crate::{aug, col_by_id, col_of, folder_size_dio, hydrated, open, redb_lens};

/// Refresh re-runs the list pass and reconciles against the cache without
/// discarding still-valid augmentation: an unchanged master leaves hydrated
/// detail columns intact (the property that makes auto-refresh safe).
#[tokio::test]
async fn refresh_keeps_hydrated_detail_when_master_unchanged() {
    let tmp = TempDir::new().unwrap();
    let dio = open(&tmp, vec![aug("runs-detail", Source::Id, &["detail"])]).await;
    let scenery = hydrated(&dio, 0).await;

    dio.refresh().await.expect("refresh");
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Reconciliation kept the row's augmented column rather than wiping it.
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
}

/// Staleness: when a base row's list fields move (its `modified` bumps), the
/// refresh reconciliation demotes it and the standing viewport refetches the
/// augment — the row shows the NEW detail value, not the stale hydration.
#[tokio::test]
async fn changed_master_row_refetches_its_augment() {
    let tmp = TempDir::new().unwrap();

    // MockShell clones share one store — the writer handles mutate what the
    // dio's master and fixed detail read.
    let master_shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("modified", "t1")]))
        .with_metadata(meta(&["id", "modified"]));
    let detail_shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("size", "100")]))
        .with_metadata(meta(&["id", "size"]));
    let master_writer = Vista::new("m", master_shell.clone());
    let detail_writer = Vista::new("d", detail_shell.clone());

    let dio = augmented_dio(
        &redb_lens(&tmp),
        Vista::new("listing", master_shell),
        vec![fixed_augmentation(
            Vista::new("sizes", detail_shell),
            &["size"],
        )],
    )
    .await;
    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..1, 0..0);
    poll_until("hydrated with the first size", || {
        col_of(&scenery, 0, "size").as_deref() == Some("100")
    })
    .await;

    // The world moves: the folder's modified bumps and its size grows.
    detail_writer
        .replace_value("r0", &record(&[("id", "r0"), ("size", "200")]))
        .await
        .expect("detail write");
    master_writer
        .replace_value("r0", &record(&[("id", "r0"), ("modified", "t2")]))
        .await
        .expect("master write");

    // Reconcile (demotes r0 — a list field changed), then let the standing
    // viewport re-run the detail pass for the demoted row.
    dio.refresh().await.expect("refresh");
    scenery.set_viewport(0..1, 0..0);

    poll_until("augment refetched", || {
        col_of(&scenery, 0, "size").as_deref() == Some("200")
            && col_of(&scenery, 0, "modified").as_deref() == Some("t2")
    })
    .await;
}

// ---------------------------------------------------------------------------
// Reconcile must not flap augmented columns (live_folder regression): the
// LIST row itself carries a cheap `size` (0 for folders) that the augment
// overwrites. A refresh whose list fields are otherwise unchanged must keep
// the augmented value and fetch NOTHING; a genuinely-changed row (its
// `modified` moved) refetches — and only that row.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_with_unchanged_list_rows_keeps_augment_and_fetches_nothing() {
    let tmp = TempDir::new().unwrap();

    // The live_folder listing shape: FOLDER rows leave the augment column
    // unfilled (their recursive size is the augment's job); the FILE row
    // carries its own size from the list — no gap, nothing to fetch.
    let master_shell = MockShell::new()
        .with_record("logs", record(&[("id", "logs"), ("modified", "t1")]))
        .with_record("tmp", record(&[("id", "tmp"), ("modified", "t1")]))
        .with_record(
            "run.log",
            record(&[("id", "run.log"), ("modified", "t1"), ("size", "64")]),
        )
        .with_metadata(meta(&["id", "modified", "size"]));
    let master_writer = Vista::new("m", master_shell.clone());

    let (dio, detail) = folder_size_dio(&tmp, master_shell).await;
    let gets = || detail.get_log().len();
    let scenery = dio.table_scenery().page_size(4).open().await.unwrap();
    // Row order shifts as reseeds re-list the cache — address rows by id.
    let by_id = |id: &'static str, c: &'static str| {
        let scenery = scenery.clone();
        move || col_by_id(&scenery, id, c)
    };
    scenery.set_viewport(0..3, 0..0);
    poll_until("folder rows hydrated", || {
        by_id("logs", "size")().as_deref() == Some("4096")
            && by_id("tmp", "size")().as_deref() == Some("512")
    })
    .await;
    assert_eq!(
        gets(),
        2,
        "gap rule: only the two folders fetched — the file's list-supplied size is no gap"
    );
    assert_eq!(
        by_id("run.log", "size")().as_deref(),
        Some("64"),
        "the file keeps its own listed size"
    );

    // Two refresh "ticks" with NOTHING changed. The augmented value must
    // survive, no row may demote, and zero detail fetches may fire.
    for _ in 0..2 {
        dio.refresh().await.expect("refresh");
        scenery.set_viewport(0..3, 0..0);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            by_id("logs", "size")().as_deref(),
            Some("4096"),
            "unchanged row keeps its augmented value through refresh"
        );
        assert_eq!(by_id("tmp", "size")().as_deref(), Some("512"));
    }
    assert_eq!(gets(), 2, "unchanged rows never refetch their augment");

    // One folder's `modified` moves: its augment clears (the gap is open
    // again) and ONLY that row refetches. A moved FILE row never fetches —
    // its list row still has no gap.
    master_writer
        .replace_value("logs", &record(&[("id", "logs"), ("modified", "t2")]))
        .await
        .expect("master write");
    master_writer
        .replace_value(
            "run.log",
            &record(&[("id", "run.log"), ("modified", "t2"), ("size", "96")]),
        )
        .await
        .expect("file write");
    dio.refresh().await.expect("refresh");
    scenery.set_viewport(0..3, 0..0);
    // The view may reflect the reconciled cache (stale size kept) before the
    // async refetch lands — wait on the fetch counter as well, not just the
    // visible values.
    poll_until("changed folder refetched, file re-listed", || {
        by_id("logs", "modified")().as_deref() == Some("t2")
            && by_id("logs", "size")().as_deref() == Some("4096")
            && by_id("run.log", "size")().as_deref() == Some("96")
            && gets() == 3
    })
    .await;
    assert_eq!(
        gets(),
        3,
        "exactly the changed folder refetched; its sibling and the file stayed cold"
    );
    assert_eq!(by_id("tmp", "size")().as_deref(), Some("512"));
}
