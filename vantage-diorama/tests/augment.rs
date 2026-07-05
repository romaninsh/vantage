//! Generic augmentation, end-to-end: a master Vista listed cheaply, each row
//! enriched one-at-a-time from a *separate* detail Vista resolved through a
//! [`VistaCatalog`]. Proves the `.augment(...)` wiring drives the two-pass
//! machinery without any hand-written list/detail callbacks.
//!
//! Both vistas are in-memory `MockShell`s — independent handles resolved by
//! name, which is exactly the "wire two generic Vistas into one Dio" shape (the
//! same path serves a REST master + cmd detail, since the catalog is
//! persistence-agnostic).

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_diorama::{
    Augmentation, Detail, Dio, Fetch, Lens, MergeRule, RowStatus, Source, TableScenery,
};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};
use vantage_vista_factory::VistaCatalog;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), text(v)))
        .collect()
}

fn meta(columns: &[&str]) -> VistaMetadata {
    let mut m = VistaMetadata::new();
    for c in columns {
        let col = if *c == "id" {
            Column::new("id", "String").with_flag("id")
        } else {
            Column::new(*c, "String")
        };
        m = m.with_column(col);
    }
    m.with_id_column("id")
}

/// Master: two rows with cheap columns only (id, branch).
fn master_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("branch", "main")]))
        .with_record("r1", record(&[("id", "r1"), ("branch", "dev")]))
        .with_metadata(meta(&["id", "branch"]));
    Vista::new("runs", Box::new(shell))
}

/// Detail source #1: keyed by id, carries the expensive `detail` column.
fn detail_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("detail", "full-r0")]))
        .with_record("r1", record(&[("id", "r1"), ("detail", "full-r1")]))
        .with_metadata(meta(&["id", "detail"]));
    Vista::new("runs-detail", Box::new(shell))
}

/// Detail source #2: a *second* independent vista, carries `extra`.
fn extra_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("extra", "x0")]))
        .with_record("r1", record(&[("id", "r1"), ("extra", "x1")]))
        .with_metadata(meta(&["id", "extra"]));
    Vista::new("runs-extra", Box::new(shell))
}

fn catalog() -> Arc<VistaCatalog> {
    let mut c = VistaCatalog::new();
    c.register("runs-detail", Arc::new(|| Ok(detail_vista())));
    c.register("runs-extra", Arc::new(|| Ok(extra_vista())));
    Arc::new(c)
}

fn aug(table: &str, source: Source, merge: &[&str]) -> Augmentation {
    Augmentation {
        detail: Detail::Catalog(table.into()),
        source,
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: merge.iter().map(|s| s.to_string()).collect(),
        },
    }
}

async fn open(tmp: &TempDir, augmentations: Vec<Augmentation>) -> Dio {
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .viewport_debounce(Duration::from_millis(1))
            .build()
            .expect("lens builds"),
    );
    let dio = lens.make_dio(master_vista()).await.expect("make_dio");
    dio.augment(catalog(), augmentations)
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

// ---------------------------------------------------------------------------

/// The list pass stubs rows `Incomplete` with cheap columns; no detail fetched
/// until a viewport asks. Then the detail pass merges the second vista's column
/// and flips the row `Fresh`, keeping the cheap columns.
#[tokio::test]
async fn id_source_augments_from_a_separate_vista() {
    let tmp = TempDir::new().unwrap();
    let dio = open(&tmp, vec![aug("runs-detail", Source::Id, &["detail"])]).await;
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();

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
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);

    eventually("hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
    })
    .await;
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
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);

    eventually("hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
    })
    .await;

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
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
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
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);
    eventually("hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
    })
    .await;

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
        detail: Detail::Catalog("runs-detail".into()),
        source: Source::Id,
        fetch: Fetch::Custom(Arc::new(|detail: V| {
            Box::pin(async move { Ok(detail.list_values().await?.into_values().collect()) })
        })),
        merge: MergeRule {
            columns: vec!["detail".into()],
        },
    };
    let dio = open(&tmp, vec![augmentation]).await;
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);

    eventually("hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
    })
    .await;
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
}

/// End-to-end through the Rhai-scripted source path: a `script` spec lowers to a
/// `Build` closure that narrows the detail vista using the master `row`.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn scripted_source_augments_via_rhai() {
    use vantage_diorama::{AugmentSpec, FetchSpec, SourceSpec, lower_augment};

    let tmp = TempDir::new().unwrap();
    let spec = AugmentSpec {
        table: "runs-detail".into(),
        source: SourceSpec::Script {
            code: r#"self.add_condition_eq("id", row.id)"#.into(),
        },
        fetch: FetchSpec::PerRow,
        merge: vec!["detail".into()],
    };
    let augmentation = lower_augment(spec, &catalog()).expect("lowers with rhai");
    let dio = open(&tmp, vec![augmentation]).await;
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);

    eventually("both rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(col_of(&scenery, 1, "detail").as_deref(), Some("full-r1"));
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
    let fixed = Arc::new(Vista::new("folder_size", Box::new(size_shell)));

    let master = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("path", "a/logs")]))
        .with_record("r1", record(&[("id", "r1"), ("path", "a/tmp")]))
        .with_metadata(meta(&["id", "path"]));
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .viewport_debounce(Duration::from_millis(1))
            .build()
            .expect("lens builds"),
    );
    let dio = lens
        .make_dio(Vista::new("listing", Box::new(master)))
        .await
        .expect("make_dio")
        .augment(
            Arc::new(VistaCatalog::new()), // EMPTY — the fixed handle needs no catalog
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
        );
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
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
    let master_writer = Vista::new("m", Box::new(master_shell.clone()));
    let detail_writer = Vista::new("d", Box::new(detail_shell.clone()));

    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .viewport_debounce(Duration::from_millis(1))
            .build()
            .expect("lens builds"),
    );
    let dio = lens
        .make_dio(Vista::new("listing", Box::new(master_shell)))
        .await
        .expect("make_dio")
        .augment(
            Arc::new(VistaCatalog::new()),
            vec![Augmentation {
                detail: Detail::Fixed(Arc::new(Vista::new("sizes", Box::new(detail_shell)))),
                source: Source::Id,
                fetch: Fetch::PerRow,
                merge: MergeRule {
                    columns: vec!["size".into()],
                },
            }],
        );
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
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
