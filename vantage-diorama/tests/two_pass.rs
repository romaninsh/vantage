//! Two-pass (progressive) loading, end-to-end, wired to **real** vantage-cmd
//! list + detail scripts (a shell fixture — no mocked records).
//!
//! These tests assert the things that matter for progressive loading:
//! invocation **count**, invocation **order**, and async correctness. They do
//! NOT simulate slow loads or advance a paused clock (the cucumber suite's
//! flakiness comes from clock-advance); instead they run on a real clock with a
//! tiny viewport debounce and a bounded poll helper.
//!
//! The fixture (`fixtures/two_pass_runs.sh`) logs every invocation to
//! `$RUNS_LOG`, so a test reads that file back to assert the exact sequence of
//! `list`/`detail` calls the machinery issued.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_cmd::{Cmd, CmdSpec, eq};
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Dio, Lens, RowStatus, SortDir, TableScenery};
use vantage_table::pagination::Pagination;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

const ENTITY: &str = "gh-workflow-runs";

/// List script: build `["list", offset, limit, <branch?>]` from scope and parse
/// the JSON the fixture emits.
const LIST: &str = r#"
    let args = ["list", offset, limit];
    for c in conditions { args += [c.value]; }
    parse_json(run(args).stdout)
"#;

/// Detail script: run the fixture with `id` in scope.
const DETAIL: &str = r#"parse_json(run(["detail", id]).stdout)"#;

fn fixture() -> String {
    format!(
        "{}/tests/fixtures/two_pass_runs.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// A `Cmd` over the fixture, with the list + detail scripts and the log path
/// baked into the environment.
fn make_cmd(log: &Path) -> Cmd {
    Cmd::new(fixture())
        .with_env("RUNS_LOG", log.to_string_lossy().to_string())
        .with_table(ENTITY, CmdSpec::new(LIST).with_detail(DETAIL))
}

/// Build a master `Vista` over the cmd source — used by the Dio only for its
/// name (cache table key) and `index_key`.
fn master(cmd: &Cmd) -> vantage_vista::Vista {
    let table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd.clone())
        .with_id_column("id")
        .with_column_of::<String>("branch")
        .with_column_of::<String>("detail");
    cmd.vista_factory().from_table(table).expect("master vista")
}

fn cbor_text(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

/// Build a two-pass Lens: `on_list_page` runs the cmd list script for the
/// requested window/conditions; `on_load_detail` runs the cmd detail script for
/// one id. Registering `on_load_detail` is what opts the Dio into two-pass.
fn two_pass_lens(cache_dir: &Path, log: &Path) -> Arc<Lens> {
    let list_cmd = make_cmd(log);
    let detail_cmd = make_cmd(log);
    let lens = Lens::new()
        .cache_at(cache_dir.join("cache.redb"))
        .viewport_debounce(Duration::from_millis(1))
        .on_list_page(move |_dio, q| {
            let cmd = list_cmd.clone();
            async move {
                let mut table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd)
                    .with_id_column("id")
                    .with_column_of::<String>("branch");
                for (field, value) in &q.conditions {
                    table.add_condition(eq(field.clone(), value.clone()));
                }
                let limit = q.limit.max(1) as i64;
                let page = (q.offset as i64 / limit) + 1;
                table.set_pagination(Some(Pagination::new(page, limit)));
                let rows = table.list_values().await?;
                Ok(rows.into_iter().collect::<Vec<_>>())
            }
        })
        .on_load_detail(move |_dio, id| {
            let cmd = detail_cmd.clone();
            async move {
                let table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd)
                    .with_id_column("id")
                    .with_column_of::<String>("branch")
                    .with_column_of::<String>("detail");
                table
                    .get_value(&id)
                    .await?
                    .ok_or_else(|| vantage_core::error!("detail: no record for id"))
            }
        })
        .build()
        .expect("lens builds");
    Arc::new(lens)
}

/// Read the fixture's invocation log back as lines.
fn log_lines(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .collect()
}

fn count_with_prefix(log: &Path, prefix: &str) -> usize {
    log_lines(log)
        .iter()
        .filter(|l| l.starts_with(prefix))
        .count()
}

/// Poll until `f` holds, or panic after ~1s. Real clock; small sleeps yield to
/// the spawned viewport / list tasks.
async fn eventually(label: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition '{label}' not met within timeout");
}

fn status_of(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<RowStatus> {
    scenery.row(idx).map(|r| r.status.clone())
}

fn detail_of(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery
        .row(idx)
        .and_then(|r| r.record.get("detail").and_then(cbor_text))
}

fn branch_of(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery
        .row(idx)
        .and_then(|r| r.record.get("branch").and_then(cbor_text))
}

async fn open_dio(lens: &Arc<Lens>, cmd: &Cmd) -> Dio {
    lens.make_dio(master(cmd)).await.expect("make_dio")
}

// ---------------------------------------------------------------------------

/// Opening a two-pass scenery runs the list pass exactly once and yields
/// `Incomplete` rows carrying the cheap columns — and issues **zero** detail
/// calls until a viewport asks for them.
#[tokio::test]
async fn list_pass_creates_incomplete_rows_and_no_detail_calls() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path(), &log);
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();

    // First list page already ran (open awaits it): r0, r1 as Incomplete.
    assert_eq!(scenery.estimated_total(), Some(2));
    assert!(scenery.has_more(), "a full first page implies more pages");
    assert_eq!(
        status_of(&scenery, 0).map(|s| matches!(s, RowStatus::Incomplete)),
        Some(true)
    );
    assert_eq!(branch_of(&scenery, 0).as_deref(), Some("main"));
    assert!(
        detail_of(&scenery, 0).is_none(),
        "no detail before the detail pass"
    );

    // Exactly one list call, no detail calls.
    assert_eq!(
        count_with_prefix(&log, "list"),
        1,
        "log: {:?}",
        log_lines(&log)
    );
    assert_eq!(count_with_prefix(&log, "detail"), 0);
}

/// A viewport hydrates each visible `Incomplete` row exactly once, in order,
/// flipping it to `Fresh`. Re-entering the same viewport hydrates nothing.
#[tokio::test]
async fn detail_pass_hydrates_visible_rows_once_in_order() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path(), &log);
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);

    eventually("rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;

    assert_eq!(detail_of(&scenery, 0).as_deref(), Some("full-r0"));
    assert_eq!(detail_of(&scenery, 1).as_deref(), Some("full-r1"));
    assert_eq!(
        branch_of(&scenery, 0).as_deref(),
        Some("main"),
        "merge keeps cheap cols"
    );

    // Two detail calls, in row order.
    let details: Vec<String> = log_lines(&log)
        .into_iter()
        .filter(|l| l.starts_with("detail"))
        .collect();
    assert_eq!(details, vec!["detail id=r0", "detail id=r1"]);

    // Re-entering the same viewport must not re-hydrate.
    scenery.set_viewport(0..2);
    eventually("debounce settles", || true).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(count_with_prefix(&log, "detail"), 2, "no re-hydration");
}

/// Regression (soft-refresh sort): changing the sort **restarts the detail
/// pass for the visible window without a scroll**. Before the fix, `set_sort`
/// dropped a two-pass scenery onto the single-pass reseed path, which never
/// rebuilt the ordered index nor re-issued the viewport — so augmentation
/// silently stopped until the user happened to scroll.
#[tokio::test]
async fn sort_change_restarts_augmentation_without_scrolling() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path(), &log);
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);
    eventually("initial hydration", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;
    let list_before = count_with_prefix(&log, "list");

    // Change the sort in place — NO new set_viewport. The machinery must rebuild
    // the ordered index for the new variant (one list page) and restart the
    // detail pass on its own. Before the fix, set_sort fell to the single-pass
    // reseed-from-cache path: it never listed the new variant nor re-issued the
    // viewport, so `list` stayed flat and augmentation was dead.
    scenery.set_sort(Some("branch".to_string()), SortDir::Asc);

    eventually("new sort variant is listed", || {
        count_with_prefix(&log, "list") > list_before
            && matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
    })
    .await;

    assert!(
        count_with_prefix(&log, "list") > list_before,
        "a sort change must list the new variant, not silently reseed from cache"
    );
    // Soft-refresh: the visible row never blanks to None, and its augmented
    // (detail) columns survive the reorder because the cache is keyed by id.
    assert!(scenery.row(0).is_some(), "row 0 stays present across the resort");
    assert_eq!(detail_of(&scenery, 0).as_deref(), Some("full-r0"));

    // Reverting to a variant already listed reorders straight from cache — its
    // index exists and its rows are `Complete`, so neither a list nor a detail
    // call is issued, and the grid never blanks.
    let list_after_sort = count_with_prefix(&log, "list");
    let detail_after_sort = count_with_prefix(&log, "detail");
    scenery.set_sort(None, SortDir::Asc);
    eventually("revert reseeds from cache", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        count_with_prefix(&log, "list"),
        list_after_sort,
        "reverting to a seen variant must not re-list it"
    );
    assert_eq!(
        count_with_prefix(&log, "detail"),
        detail_after_sort,
        "reverting must not re-fetch already-complete details"
    );
    assert!(scenery.row(0).is_some(), "row 0 never blanks on revert");
}

/// Sequential, no-total paging: each `request_load_more` fetches the next list
/// page; a short final page flips `has_more` false and freezes the estimate.
#[tokio::test]
async fn sequential_paging_stops_on_short_page() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path(), &log);
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    assert_eq!(scenery.estimated_total(), Some(2));
    assert!(scenery.has_more());

    // Page 2: r2, r3 (full page) — still more.
    scenery.request_load_more();
    eventually("page 2 loaded", || scenery.estimated_total() == Some(4)).await;
    assert!(scenery.has_more());

    // Page 3: r4 only (short page) — paging ends, estimate freezes at 5.
    scenery.request_load_more();
    eventually("page 3 loaded", || scenery.estimated_total() == Some(5)).await;
    assert!(!scenery.has_more(), "short page ends paging");

    // A further load_more must not issue another list call.
    let lists_before = count_with_prefix(&log, "list");
    scenery.request_load_more();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        count_with_prefix(&log, "list"),
        lists_before,
        "no paging past the end"
    );
    assert_eq!(scenery.estimated_total(), Some(5));
    assert_eq!(
        count_with_prefix(&log, "detail"),
        0,
        "paging never triggers detail"
    );
}

/// Switching filter variants reuses the shared detail cache: a filtered variant
/// finds already-hydrated rows `Fresh` (no detail re-fetch), and switching back
/// to a previously-built variant reuses its cached index (no list re-fetch).
#[tokio::test]
async fn shared_detail_across_filter_variants() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path(), &log);
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    // Unfiltered: page through all 5 and hydrate them.
    let all = dio.table_scenery().page_size(2).open().await.unwrap();
    all.request_load_more();
    eventually("page2", || all.estimated_total() == Some(4)).await;
    all.request_load_more();
    eventually("page3", || all.estimated_total() == Some(5)).await;
    all.set_viewport(0..5);
    eventually("all hydrated", || {
        (0..5).all(|i| matches!(status_of(&all, i), Some(RowStatus::Fresh)))
    })
    .await;
    assert_eq!(
        count_with_prefix(&log, "detail"),
        5,
        "five rows hydrated once each"
    );
    let lists_after_unfiltered = count_with_prefix(&log, "list");

    // Filtered to branch=main: a new index (list runs for the main variant),
    // but its rows are already Fresh in the shared detail cache → no detail.
    let main = dio
        .table_scenery()
        .where_eq("branch", "main")
        .page_size(2)
        .open()
        .await
        .unwrap();
    main.set_viewport(0..2);
    eventually("filtered rows present", || status_of(&main, 0).is_some()).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        matches!(status_of(&main, 0), Some(RowStatus::Fresh)),
        "filtered rows reuse the shared detail cache"
    );
    assert_eq!(detail_of(&main, 0).as_deref(), Some("full-r0"));
    assert_eq!(
        count_with_prefix(&log, "detail"),
        5,
        "no detail re-fetch across variants"
    );
    assert!(
        count_with_prefix(&log, "list") > lists_after_unfiltered,
        "filtered variant builds its own index"
    );

    // Switch back to unfiltered: same index_key → cached index reused, so
    // opening issues zero list calls and zero detail calls.
    let lists_before_switchback = count_with_prefix(&log, "list");
    let again = dio.table_scenery().page_size(2).open().await.unwrap();
    again.set_viewport(0..5);
    eventually("switchback rows present", || status_of(&again, 4).is_some()).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(again.estimated_total(), Some(5), "reused index is complete");
    assert_eq!(
        count_with_prefix(&log, "list"),
        lists_before_switchback,
        "reused index ⇒ zero list calls"
    );
    assert_eq!(
        count_with_prefix(&log, "detail"),
        5,
        "reused index ⇒ zero detail calls"
    );
}

/// Persisted completeness survives a restart: reopening against the same cache
/// file resumes the detail pass only for rows that are not already `Complete`.
#[tokio::test]
async fn reopen_resumes_without_refetching_complete_rows() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();

    // First run: hydrate the first page, then drop everything.
    {
        let log = cache_dir.join("run1.log");
        let lens = two_pass_lens(&cache_dir, &log);
        let dio = open_dio(&lens, &make_cmd(&log)).await;
        let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
        scenery.set_viewport(0..2);
        eventually("run1 hydrated", || {
            matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
                && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
        })
        .await;
    }

    // Second run: a fresh log against the SAME cache file. The list pass rebuilds
    // the index, but the persisted `Complete` rows mean zero detail calls.
    let log2 = cache_dir.join("run2.log");
    let lens2 = two_pass_lens(&cache_dir, &log2);
    let dio2 = open_dio(&lens2, &make_cmd(&log2)).await;
    let scenery2 = dio2.table_scenery().page_size(2).open().await.unwrap();

    // Rows come back `Fresh` straight from the persisted cache.
    eventually("run2 rows present", || status_of(&scenery2, 0).is_some()).await;
    assert!(matches!(status_of(&scenery2, 0), Some(RowStatus::Fresh)));
    assert_eq!(detail_of(&scenery2, 0).as_deref(), Some("full-r0"));

    scenery2.set_viewport(0..2);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        count_with_prefix(&log2, "detail"),
        0,
        "already-complete rows are never re-fetched after restart"
    );
}

/// A detail-pass failure for one id marks only that row failed; the others
/// hydrate normally.
#[tokio::test]
async fn detail_failure_marks_only_that_row() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");

    // Reuse the two-pass lens but bake FAIL_ID into the detail cmd so r1 errors.
    let list_cmd = make_cmd(&log);
    let detail_cmd = make_cmd(&log)
        .with_env("FAIL_ID", "r1")
        .with_table(ENTITY, CmdSpec::new(LIST).with_detail(DETAIL));
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .viewport_debounce(Duration::from_millis(1))
            .on_list_page(move |_dio, q| {
                let cmd = list_cmd.clone();
                async move {
                    let mut table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd)
                        .with_id_column("id")
                        .with_column_of::<String>("branch");
                    for (field, value) in &q.conditions {
                        table.add_condition(eq(field.clone(), value.clone()));
                    }
                    let limit = q.limit.max(1) as i64;
                    let page = (q.offset as i64 / limit) + 1;
                    table.set_pagination(Some(Pagination::new(page, limit)));
                    Ok(table.list_values().await?.into_iter().collect::<Vec<_>>())
                }
            })
            .on_load_detail(move |_dio, id| {
                let cmd = detail_cmd.clone();
                async move {
                    let table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd)
                        .with_id_column("id")
                        .with_column_of::<String>("branch")
                        .with_column_of::<String>("detail");
                    table
                        .get_value(&id)
                        .await?
                        .ok_or_else(|| vantage_core::error!("detail: no record"))
                }
            })
            .build()
            .expect("lens"),
    );

    let dio = open_dio(&lens, &make_cmd(&log)).await;
    let scenery = dio.table_scenery().page_size(2).open().await.unwrap();
    scenery.set_viewport(0..2);

    eventually("r0 fresh, r1 failed", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::LoadFailed { .. }))
    })
    .await;

    assert_eq!(detail_of(&scenery, 0).as_deref(), Some("full-r0"));
    // The failed row keeps its cheap list column and is not Fresh.
    assert_eq!(branch_of(&scenery, 1).as_deref(), Some("dev"));
}

/// Opt-in guard: with no `on_load_detail`, the scenery uses the legacy
/// single-pass path — rows are `Fresh` immediately and no index is built.
#[tokio::test]
async fn no_detail_callback_uses_single_pass() {
    use vantage_dataset::prelude::ReadableValueSet as _;

    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let load_cmd = make_cmd(&log);

    // Legacy lens: on_start copies the full list into the cache; no detail.
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_start(move |dio| {
                let dio = dio.clone();
                let cmd = load_cmd.clone();
                async move {
                    let table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd)
                        .with_id_column("id")
                        .with_column_of::<String>("branch");
                    let rows = table.list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("lens"),
    );

    let dio = open_dio(&lens, &make_cmd(&log)).await;
    let scenery = dio.table_scenery().page_size(50).open().await.unwrap();

    // Single-pass: every cached row is Fresh, no Incomplete stubs.
    assert_eq!(scenery.row_count(), 5);
    for i in 0..5 {
        assert!(
            matches!(status_of(&scenery, i), Some(RowStatus::Fresh)),
            "row {i} should be Fresh in single-pass mode"
        );
    }
    assert_eq!(
        count_with_prefix(&log, "detail"),
        0,
        "no detail script in single-pass"
    );
}
