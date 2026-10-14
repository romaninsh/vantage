//! Several views over one two-pass Dio: a `titles_only` picker beside a full
//! grid, what diagnostics report about them, and the detail cache shared
//! across filter variants.

use std::time::Duration;

use tempfile::TempDir;
use vantage_diorama::RowStatus;
use vantage_vista::{Condition, FilterOp};

use crate::support::{self, poll_until, status_of};
use crate::{branch_of, count_with_prefix, detail_of, make_cmd, open_dio, two_pass_lens};

/// A `titles_only` picker (dropdown/autocomplete) over an augmented table lists
/// the cheap columns and **never hydrates detail** — even as its viewport
/// moves — while a full grid over the same query is a distinct scenery that
/// does hydrate. The picker pays list cost only.
#[tokio::test]
async fn titles_only_picker_skips_detail_hydration() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path());
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let picker = dio
        .table_scenery()
        .titles_only()
        .page_size(2)
        .open()
        .await
        .unwrap();
    support::wait_listed(&picker).await;
    picker.set_viewport(0..2, 0..0);
    // Let the viewport debounce + (suppressed) detail path run.
    tokio::time::sleep(Duration::from_millis(30)).await;

    // List columns are present; the row stays Incomplete and no detail ran.
    assert_eq!(
        branch_of(&picker, 0).as_deref(),
        Some("main"),
        "title column listed"
    );
    assert!(
        matches!(status_of(&picker, 0), Some(RowStatus::Incomplete)),
        "picker rows stay Incomplete: {:?}",
        status_of(&picker, 0)
    );
    assert!(detail_of(&picker, 0).is_none(), "no detail column");
    assert_eq!(
        count_with_prefix(&log, "detail"),
        0,
        "picker must not hydrate"
    );
    assert_eq!(count_with_prefix(&log, "list"), 1, "one list page");

    // A full grid over the same query is a DISTINCT scenery (it hydrates), so
    // the Dio now holds two live sceneries.
    let grid = support::open_listed(&dio, 2).await;
    assert!(
        !std::sync::Arc::ptr_eq(&picker, &grid),
        "titles_only keys distinctly from a full grid"
    );
    #[cfg(feature = "diagnostics")]
    assert_eq!(
        dio.diagnostics()
            .await
            .expect("diagnostics")
            .sceneries
            .len(),
        2
    );

    grid.set_viewport(0..2, 0..0);
    poll_until("grid hydrates", || {
        matches!(status_of(&grid, 0), Some(RowStatus::Fresh))
    })
    .await;
    assert!(
        count_with_prefix(&log, "detail") > 0,
        "the full grid hydrates even though the picker didn't"
    );
}

/// `dio.diagnostics()` reports every open view with how many views share its
/// rows and their hydration breakdown, and the cache size — and empties as
/// handles are released.
#[cfg(feature = "diagnostics")]
#[tokio::test]
async fn diagnostics_report_sceneries_refcount_and_hydration() -> vantage_core::Result<()> {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path());
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    // A picker (titles_only) lists cheap rows — Incomplete, never hydrated.
    let picker = dio
        .table_scenery()
        .titles_only()
        .page_size(2)
        .open()
        .await
        .unwrap();
    // Wait for the rows themselves, not the index: a two-pass `row_count()`
    // reads the list-pass index, while `status_summary` counts the sparse map
    // the assertions below read. The two are filled by different steps, so
    // waiting on the index let this pass with one row materialised.
    poll_until("picker rows materialised", || {
        picker.status_summary().loaded >= 2
    })
    .await;

    let d = dio.diagnostics().await?;
    assert!(!d.live, "built without DioBuilder::live");
    assert_eq!(d.sceneries.len(), 1);
    assert_eq!(d.sceneries[0].shared_by, 1);
    assert!(
        d.sceneries[0].status.incomplete >= 2,
        "picker rows are Incomplete: {:?}",
        d.sceneries[0].status
    );
    assert_eq!(d.sceneries[0].status.fresh, 0);

    // Re-opening the same picker query is a second view over the same rows:
    // two entries, each shared by two.
    let picker2 = dio
        .table_scenery()
        .titles_only()
        .page_size(2)
        .open()
        .await
        .unwrap();
    let d = dio.diagnostics().await?;
    assert_eq!(d.sceneries.len(), 2);
    assert!(
        d.sceneries.iter().all(|s| s.shared_by == 2),
        "{:?}",
        d.sceneries
    );

    // A full grid reads distinct rows that hydrate → Fresh rows.
    let grid = support::open_listed(&dio, 2).await;
    grid.set_viewport(0..2, 0..0);
    poll_until("grid hydrates both rows", || {
        matches!(status_of(&grid, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&grid, 1), Some(RowStatus::Fresh))
    })
    .await;

    let d = dio.diagnostics().await?;
    assert_eq!(d.sceneries.len(), 3, "two pickers + grid");
    assert!(d.augmented_rows() >= 2, "grid contributes Fresh rows");
    assert!(d.cache_rows >= 2);

    // Releasing every handle empties the diagnostics (no leak).
    drop(picker);
    drop(picker2);
    drop(grid);
    let d = dio.diagnostics().await?;
    assert_eq!(d.sceneries.len(), 0, "released sceneries are not reported");
    Ok(())
}

/// Switching filter variants reuses the shared detail cache: a filtered variant
/// finds already-hydrated rows `Fresh` (no detail re-fetch), and switching back
/// to a previously-built variant reuses its cached index (no list re-fetch).
#[tokio::test]
async fn shared_detail_across_filter_variants() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path());
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    // Unfiltered: page through all 5 and hydrate them.
    let all = support::open_listed(&dio, 2).await;
    all.request_load_more();
    poll_until("page2", || all.estimated_total() == Some(4)).await;
    all.request_load_more();
    poll_until("page3", || all.estimated_total() == Some(5)).await;
    all.set_viewport(0..5, 0..0);
    poll_until("all hydrated", || {
        (0..5).all(|i| matches!(status_of(&all, i), Some(RowStatus::Fresh)))
    })
    .await;
    assert_eq!(
        count_with_prefix(&log, "detail"),
        5,
        "five rows hydrated once each"
    );
    let lists_after_unfiltered = count_with_prefix(&log, "list");

    // Filtered to branch=main: a new index (list runs for the main variant:
    // the paged listing above is no whole copy), but its rows are already
    // Fresh in the shared detail cache → no detail.
    let main = dio
        .table_scenery()
        .filter(Condition::new("branch", FilterOp::Eq, "main"))
        .page_size(2)
        .open()
        .await
        .unwrap();
    main.set_viewport(0..2, 0..0);
    poll_until("filtered rows present", || status_of(&main, 0).is_some()).await;
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
    assert!(main.row_count() > 0);
    assert!(
        (0..main.row_count()).all(|i| branch_of(&main, i).as_deref() == Some("main")),
        "the rows are refined to main: {:?}",
        (0..main.row_count())
            .map(|i| branch_of(&main, i))
            .collect::<Vec<_>>()
    );

    // Switch back to unfiltered: same index_key → cached index reused, so
    // opening issues zero list calls and zero detail calls.
    let lists_before_switchback = count_with_prefix(&log, "list");
    let again = dio.table_scenery().page_size(2).open().await.unwrap();
    again.set_viewport(0..5, 0..0);
    poll_until("switchback rows present", || status_of(&again, 4).is_some()).await;
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
