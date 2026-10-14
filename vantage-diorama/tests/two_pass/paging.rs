//! Sequential paging, a restart over the same cache file, and the
//! single-pass path a Dio without augmentation keeps.

use std::time::Duration;

use tempfile::TempDir;
use vantage_cmd::Cmd;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, Lens, Loader, RowStatus};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

use crate::support::{self, poll_until, status_of};
use crate::{ENTITY, count_with_prefix, detail_of, make_cmd, master, open_dio, two_pass_lens};

/// Sequential, no-total paging: each `request_load_more` fetches the next list
/// page; a short final page flips `has_more` false and freezes the estimate.
#[tokio::test]
async fn sequential_paging_stops_on_short_page() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path());
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = support::open_listed(&dio, 2).await;
    assert_eq!(scenery.estimated_total(), Some(2));
    assert!(scenery.has_more());

    // Page 2: r2, r3 (full page) — still more.
    scenery.request_load_more();
    poll_until("page 2 loaded", || scenery.estimated_total() == Some(4)).await;
    assert!(scenery.has_more());

    // Page 3: r4 only (short page) — paging ends, estimate freezes at 5.
    scenery.request_load_more();
    poll_until("page 3 loaded", || scenery.estimated_total() == Some(5)).await;
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

/// Persisted completeness survives a restart: reopening against the same cache
/// file resumes the detail pass only for rows that are not already `Complete`.
#[tokio::test]
async fn reopen_resumes_without_refetching_complete_rows() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();

    // First run: hydrate the first page, then drop everything.
    {
        let log = cache_dir.join("run1.log");
        let lens = two_pass_lens(&cache_dir);
        let dio = open_dio(&lens, &make_cmd(&log)).await;
        let scenery = support::open_listed(&dio, 2).await;
        scenery.set_viewport(0..2, 0..0);
        poll_until("run1 hydrated", || {
            matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
                && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
        })
        .await;
    }

    // Second run: a fresh log against the SAME cache file. The list pass rebuilds
    // the index, but the persisted `Complete` rows mean zero detail calls.
    let log2 = cache_dir.join("run2.log");
    let lens2 = two_pass_lens(&cache_dir);
    let dio2 = open_dio(&lens2, &make_cmd(&log2)).await;
    let scenery2 = dio2.table_scenery().page_size(2).open().await.unwrap();

    // Rows come back `Fresh` straight from the persisted cache.
    poll_until("run2 rows present", || status_of(&scenery2, 0).is_some()).await;
    assert!(matches!(status_of(&scenery2, 0), Some(RowStatus::Fresh)));
    assert_eq!(detail_of(&scenery2, 0).as_deref(), Some("full-r0"));

    scenery2.set_viewport(0..2, 0..0);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        count_with_prefix(&log2, "detail"),
        0,
        "already-complete rows are never re-fetched after restart"
    );
}

/// Opt-in guard: a Dio without augmentation uses the single-pass path —
/// rows are `Fresh` immediately and no index is built.
#[tokio::test]
async fn no_augmentation_uses_single_pass() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let load_cmd = make_cmd(&log);

    // An eager start copies the full list into the cache; no detail.
    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .loader(Loader::eager(move |cx| {
            let cmd = load_cmd.clone();
            async move {
                let table = Table::<Cmd, EmptyEntity>::new(ENTITY, cmd)
                    .with_id_column("id")
                    .with_column_of::<String>("branch");
                let rows = table.list_values().await?;
                cx.cache().insert_values(rows).await
            }
        }))
        .build()
        .expect("lens");

    let dio = lens
        .dio(master(&make_cmd(&log)))
        .build()
        .await
        .expect("dio builds");
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
