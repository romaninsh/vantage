//! Generic augmentation, end-to-end: a master Vista listed cheaply, each row
//! enriched one-at-a-time from a *separate* detail Vista resolved through a
//! [`Catalog`]. Proves `DioBuilder::augment` drives the two-pass machinery
//! without any hand-written list/detail callbacks.
//!
//! Both vistas are in-memory `MockShell`s — independent handles resolved by
//! name, which is exactly the "wire two generic Vistas into one Dio" shape (the
//! same path serves a REST master + cmd detail, since the catalog is
//! persistence-agnostic).

mod support;

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use support::{augmented_dio, fixed_augmentation, meta, quick_lens, record, size_detail, text};
use tempfile::TempDir;
use vantage_diorama::{
    Augmentation, Cache, Detail, Dio, Fetch, Lens, MergeRule, RowStatus, Source, TableScenery,
};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Catalog, Column, Vista, VistaMetadata, VistaResolver};

/// Master: two rows with cheap columns only (id, branch).
fn master_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("branch", "main")]))
        .with_record("r1", record(&[("id", "r1"), ("branch", "dev")]))
        .with_metadata(meta(&["id", "branch"]));
    Vista::new("runs", shell)
}

/// Detail source #1: keyed by id, carries the expensive `detail` column.
fn detail_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("detail", "full-r0")]))
        .with_record("r1", record(&[("id", "r1"), ("detail", "full-r1")]))
        .with_metadata(meta(&["id", "detail"]));
    Vista::new("runs-detail", shell)
}

/// Detail source #2: a *second* independent vista, carries `extra`.
fn extra_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("extra", "x0")]))
        .with_record("r1", record(&[("id", "r1"), ("extra", "x1")]))
        .with_metadata(meta(&["id", "extra"]));
    Vista::new("runs-extra", shell)
}

fn catalog() -> Arc<dyn VistaResolver> {
    Arc::new(
        Catalog::new()
            .with("runs-detail", |_: &str| Ok(detail_vista()))
            .with("runs-extra", |_: &str| Ok(extra_vista())),
    )
}

fn resolved(table: &str) -> Detail {
    Detail::Resolved {
        resolver: catalog(),
        name: table.into(),
    }
}

fn aug(table: &str, source: Source, merge: &[&str]) -> Augmentation {
    Augmentation {
        detail: resolved(table),
        source,
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: merge.iter().map(|s| s.to_string()).collect(),
        },
    }
}

/// A no-loader Lens over a redb file in `tmp`.
fn redb_lens(tmp: &TempDir) -> Arc<Lens> {
    quick_lens(Cache::redb(tmp.path().join("cache.redb")))
}

async fn open(tmp: &TempDir, augmentations: Vec<Augmentation>) -> Dio {
    augmented_dio(&redb_lens(tmp), master_vista(), augmentations).await
}

fn status_of(s: &Arc<dyn TableScenery>, i: usize) -> Option<RowStatus> {
    s.row(i).map(|r| r.status.clone())
}

fn col_of(s: &Arc<dyn TableScenery>, i: usize, c: &str) -> Option<String> {
    s.row(i).and_then(|r| match r.record.get(c) {
        Some(CborValue::Text(t)) => Some(t.clone()),
        _ => None,
    })
}

async fn eventually(label: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition '{label}' not met within timeout");
}

/// Open a two-row listing over `dio`, ask for both rows, and wait until row
/// `row` is `Fresh`.
async fn hydrated(dio: &Dio, row: usize) -> Arc<dyn TableScenery> {
    let scenery = support::open_listed(dio, 2).await;
    scenery.set_viewport(0..2);
    eventually("hydrated", || {
        matches!(status_of(&scenery, row), Some(RowStatus::Fresh))
    })
    .await;
    scenery
}

// ---------------------------------------------------------------------------

/// The list pass stubs rows `Incomplete` with cheap columns; no detail fetched
/// until a viewport asks. Then the detail pass merges the second vista's column
/// and flips the row `Fresh`, keeping the cheap columns.
#[tokio::test]
async fn id_source_augments_from_a_separate_vista() {
    let tmp = TempDir::new().unwrap();
    let dio = open(&tmp, vec![aug("runs-detail", Source::Id, &["detail"])]).await;
    let scenery = support::open_listed(&dio, 2).await;

    // List pass ran: cheap columns present, Incomplete, no augmented column yet.
    assert!(matches!(
        status_of(&scenery, 0),
        Some(RowStatus::Incomplete)
    ));
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
    assert!(
        col_of(&scenery, 0, "detail").is_none(),
        "no detail before viewport"
    );

    scenery.set_viewport(0..2);
    eventually("rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;

    // Augmented column merged from runs-detail; cheap column survived.
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(col_of(&scenery, 1, "detail").as_deref(), Some("full-r1"));
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
}

/// A `Column` source keyed on an explicit master field resolves the same way.
#[tokio::test]
async fn column_source_keyed_by_field() {
    let tmp = TempDir::new().unwrap();
    let source = Source::Column {
        from: "id".into(),
        to: None,
    };
    let dio = open(&tmp, vec![aug("runs-detail", source, &["detail"])]).await;
    let scenery = hydrated(&dio, 0).await;
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
}

/// Multiple augmentations compose: each merges its own column from its own vista.
#[tokio::test]
async fn multiple_augmentations_merge_independently() {
    let tmp = TempDir::new().unwrap();
    let dio = open(
        &tmp,
        vec![
            aug("runs-detail", Source::Id, &["detail"]),
            aug("runs-extra", Source::Id, &["extra"]),
        ],
    )
    .await;
    let scenery = hydrated(&dio, 0).await;

    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(col_of(&scenery, 0, "extra").as_deref(), Some("x0"));
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
}

/// A missing key field surfaces as a failed row — the detail pass error marks
/// only that row `LoadFailed`, and its cheap columns survive.
#[tokio::test]
async fn missing_key_field_marks_row_failed() {
    let tmp = TempDir::new().unwrap();
    let source = Source::Column {
        from: "nonexistent".into(),
        to: None,
    };
    let dio = open(&tmp, vec![aug("runs-detail", source, &["detail"])]).await;
    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..2);

    eventually("row failed", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::LoadFailed { .. }))
    })
    .await;
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
}

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

/// A caller-supplied [`Fetch::Custom`] closure reads the narrowed detail vista.
#[tokio::test]
async fn custom_fetch_closure_reads_narrowed_detail() {
    use vantage_dataset::prelude::ReadableValueSet;
    use vantage_vista::Vista as V;

    let tmp = TempDir::new().unwrap();
    let augmentation = Augmentation {
        detail: resolved("runs-detail"),
        source: Source::Id,
        fetch: Fetch::Custom(Arc::new(|detail: V, _row| {
            Box::pin(async move { Ok(detail.list_values().await?.into_values().collect()) })
        })),
        merge: MergeRule {
            columns: vec!["detail".into()],
        },
    };
    let dio = open(&tmp, vec![augmentation]).await;
    let scenery = hydrated(&dio, 0).await;
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
}

/// A [`Fetch::Custom`] closure receives the listed master row beside the
/// narrowed detail vista: here it reads `branch`, a field the detail vista
/// doesn't have, the way cmd's row script reads its inputs.
#[tokio::test]
async fn custom_fetch_closure_reads_the_master_row() {
    let tmp = TempDir::new().unwrap();
    let augmentation = Augmentation {
        detail: resolved("runs-detail"),
        source: Source::Id,
        fetch: Fetch::Custom(Arc::new(|detail: Vista, row| {
            Box::pin(async move {
                assert!(detail.column("branch").is_none(), "only the row has it");
                let branch = match row.get("branch") {
                    Some(CborValue::Text(b)) => b.clone(),
                    other => panic!("the listed row carries its branch: {other:?}"),
                };
                Ok(vec![record(&[("label", &format!("on {branch}"))])])
            })
        })),
        merge: MergeRule {
            columns: vec!["label".into()],
        },
    };
    let dio = open(&tmp, vec![augmentation]).await;
    let scenery = hydrated(&dio, 1).await;
    assert_eq!(col_of(&scenery, 0, "label").as_deref(), Some("on main"));
    assert_eq!(col_of(&scenery, 1, "label").as_deref(), Some("on dev"));
}

/// The finder/live_folder shape: the detail is a FIXED get-only Vista handle,
/// registered in no catalog, keyed by a master column (`path`). Rows hydrate
/// the merged columns through the same lazy detail pass.
#[tokio::test]
async fn fixed_detail_vista_hydrates_without_a_catalog() {
    let tmp = TempDir::new().unwrap();

    // Get-only side table keyed by path — the folder-size vista shape.
    let size_meta = VistaMetadata::new()
        .with_column(Column::new("path", "String").with_flag("id"))
        .with_column(Column::new("size", "String"))
        .with_column(Column::new("file_count", "String"))
        .with_id_column("path");
    let size_shell = MockShell::new()
        .with_record(
            "a/logs",
            record(&[("path", "a/logs"), ("size", "4096"), ("file_count", "3")]),
        )
        .with_record(
            "a/tmp",
            record(&[("path", "a/tmp"), ("size", "512"), ("file_count", "1")]),
        )
        .with_metadata(size_meta);
    let fixed = Arc::new(Vista::new("folder_size", size_shell));

    let master = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("path", "a/logs")]))
        .with_record("r1", record(&[("id", "r1"), ("path", "a/tmp")]))
        .with_metadata(meta(&["id", "path"]));
    let dio = augmented_dio(
        &redb_lens(&tmp),
        Vista::new("listing", master),
        vec![Augmentation {
            detail: Detail::Fixed(fixed),
            source: Source::Column {
                from: "path".into(),
                to: None,
            },
            fetch: Fetch::PerRow,
            merge: MergeRule {
                columns: vec!["size".into(), "file_count".into()],
            },
        }],
    )
    .await;
    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..2);

    eventually("rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;
    assert_eq!(col_of(&scenery, 0, "size").as_deref(), Some("4096"));
    assert_eq!(col_of(&scenery, 0, "file_count").as_deref(), Some("3"));
    assert_eq!(col_of(&scenery, 1, "size").as_deref(), Some("512"));
    // The merge list is respected: the detail's own key column stays put.
    assert_eq!(col_of(&scenery, 0, "path").as_deref(), Some("a/logs"));
}

/// Staleness: when a base row's list fields move (its `modified` bumps), the
/// refresh reconciliation demotes it and the standing viewport refetches the
/// augment — the row shows the NEW detail value, not the stale hydration.
#[tokio::test]
async fn changed_master_row_refetches_its_augment() {
    use vantage_dataset::prelude::WritableValueSet;

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
    scenery.set_viewport(0..1);
    eventually("hydrated with the first size", || {
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
    scenery.set_viewport(0..1);

    eventually("augment refetched", || {
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
    use vantage_dataset::prelude::WritableValueSet;

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
    scenery.set_viewport(0..3);
    eventually("folder rows hydrated", || {
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
        scenery.set_viewport(0..3);
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
    scenery.set_viewport(0..3);
    // The view may reflect the reconciled cache (stale size kept) before the
    // async refetch lands — wait on the fetch counter as well, not just the
    // visible values.
    eventually("changed folder refetched, file re-listed", || {
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

/// A Dio over `master_shell` whose `size` comes from a fixed get-only detail
/// holding the `logs` and `tmp` folder sizes. The detail shell comes back too,
/// so a test can count its gets.
async fn folder_size_dio(tmp: &TempDir, master_shell: MockShell) -> (Dio, MockShell) {
    let detail = size_detail([("logs", "4096"), ("tmp", "512")]);
    let dio = augmented_dio(
        &redb_lens(tmp),
        Vista::new("listing", master_shell),
        vec![fixed_augmentation(
            Vista::new("sizes", detail.clone()),
            &["size"],
        )],
    )
    .await;
    (dio, detail)
}

/// A row's column by id, order-independent (reseeds re-list the cache).
fn col_by_id(s: &Arc<dyn TableScenery>, id: &str, c: &str) -> Option<String> {
    (0..s.row_count())
        .filter_map(|i| s.row(i))
        .find(|r| matches!(r.record.get("id"), Some(CborValue::Text(t)) if t == id))
        .and_then(|r| match r.record.get(c) {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
}

/// DEMAND gates the detail pass: a view demanding only cheap columns never
/// pays for the augment; opening a view that demands an augment column starts
/// the fetches (gap rows only); closing it stops them — a later demotion
/// leaves the augment blank until something demands it again.
#[tokio::test]
async fn augment_fetches_only_while_a_view_demands_augment_columns() {
    use vantage_dataset::prelude::WritableValueSet;

    let tmp = TempDir::new().unwrap();
    let master_shell = MockShell::new()
        .with_record("logs", record(&[("id", "logs"), ("modified", "t1")]))
        .with_record("tmp", record(&[("id", "tmp"), ("modified", "t1")]))
        .with_metadata(meta(&["id", "modified"]));
    let master_writer = Vista::new("m", master_shell.clone());

    let (dio, detail) = folder_size_dio(&tmp, master_shell).await;
    let gets = || detail.get_log().len();

    // A tree-shaped view: demands only cheap columns. Gap rows exist, a
    // viewport is held — and still nothing fetches.
    let tree = dio
        .table_scenery()
        .page_size(4)
        .columns(["id", "modified"])
        .open()
        .await
        .unwrap();
    tree.set_viewport(0..2);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        gets(),
        0,
        "no open view demands an augment column — the detail pass stays idle"
    );
    assert!(col_by_id(&tree, "logs", "size").is_none());

    // A listing opens beside it, demanding `size`: fetches begin.
    let listing = dio
        .table_scenery()
        .page_size(4)
        .columns(["id", "size", "modified"])
        .open()
        .await
        .unwrap();
    listing.set_viewport(0..2);
    eventually("augment lands once demanded", || {
        col_by_id(&listing, "logs", "size").as_deref() == Some("4096")
            && col_by_id(&listing, "tmp", "size").as_deref() == Some("512")
    })
    .await;
    assert_eq!(gets(), 2, "one fetch per gap row");

    // The listing closes: demand drains. A demoted row KEEPS its stale
    // augment value (stale-while-refetch — the display never blanks once
    // filled) and nothing refetches it while nobody demands the column.
    drop(listing);
    master_writer
        .replace_value("logs", &record(&[("id", "logs"), ("modified", "t2")]))
        .await
        .expect("master write");
    dio.refresh().await.expect("refresh");
    tree.set_viewport(0..2);
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        gets(),
        2,
        "demand drained — the demoted row does not refetch"
    );
    assert_eq!(
        col_by_id(&tree, "logs", "size").as_deref(),
        Some("4096"),
        "the demoted row keeps its stale value until re-demanded"
    );

    // Demand returns: the stale row refetches.
    let listing = dio
        .table_scenery()
        .page_size(4)
        .columns(["id", "size", "modified"])
        .open()
        .await
        .unwrap();
    listing.set_viewport(0..2);
    // The stale value already reads "4096", so the wait must be on the FETCH
    // COUNTER — the visible value merely stops being stale when it lands.
    eventually("re-demanded row refetches", || gets() == 3).await;
    assert_eq!(
        col_by_id(&listing, "logs", "size").as_deref(),
        Some("4096"),
        "refetched value replaces the stale one in place"
    );
}

/// A UI filter chip over an AUGMENTED column. The master never sees the
/// column (it exists only after hydration), so the term can't be pushed
/// down — `set_filters` refines the visible set over the cache instead,
/// and clearing restores the full set. This is what lets a grid filter
/// on a value that arrives from a second data source.
#[tokio::test]
async fn ui_filter_narrows_on_an_augmented_column() {
    let tmp = TempDir::new().unwrap();
    let dio = open(&tmp, vec![aug("runs-detail", Source::Id, &["detail"])]).await;
    let scenery = support::open_listed(&dio, 2).await;

    scenery.set_viewport(0..2);
    eventually("rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;
    assert_eq!(scenery.row_count(), 2);

    scenery.set_filters(vec![("detail".to_string(), text("full-r0"))]);
    eventually("filter narrowed the visible set", || {
        scenery.row_count() == 1
    })
    .await;
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(
        col_of(&scenery, 0, "branch").as_deref(),
        Some("main"),
        "cheap columns survive the refine"
    );

    scenery.set_filters(Vec::new());
    eventually("clearing restores the full set", || {
        scenery.row_count() == 2
    })
    .await;
}
