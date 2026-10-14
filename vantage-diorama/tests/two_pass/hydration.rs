//! The list pass, the detail pass over a viewport, a sort change that
//! restarts it, and a detail failure confined to its row.

use std::time::Duration;

use tempfile::TempDir;
use vantage_cmd::CmdSpec;
use vantage_diorama::RowStatus;
use vantage_vista::SortDirection;

use crate::support::{self, poll_until, status_of};
use crate::{
    DETAIL, ENTITY, LIST, branch_of, count_with_prefix, detail_of, log_lines, make_cmd, open_dio,
    two_pass_dio, two_pass_lens,
};

/// Opening a two-pass scenery runs the list pass exactly once and yields
/// `Incomplete` rows carrying the cheap columns — and issues **zero** detail
/// calls until a viewport asks for them.
#[tokio::test]
async fn list_pass_creates_incomplete_rows_and_no_detail_calls() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path());
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = support::open_listed(&dio, 2).await;

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
    let lens = two_pass_lens(tmp.path());
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..2, 0..0);

    poll_until("rows hydrated", || {
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
    scenery.set_viewport(0..2, 0..0);
    poll_until("debounce settles", || true).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(count_with_prefix(&log, "detail"), 2, "no re-hydration");
}

/// Regression (soft-refresh sort): changing the order **restarts the detail
/// pass for the visible window without a scroll**. Before the fix, `set_order`
/// dropped a two-pass scenery onto the single-pass reseed path, which never
/// rebuilt the ordered index nor re-issued the viewport — so augmentation
/// silently stopped until the user happened to scroll.
#[tokio::test]
async fn sort_change_restarts_augmentation_without_scrolling() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");
    let lens = two_pass_lens(tmp.path());
    let dio = open_dio(&lens, &make_cmd(&log)).await;

    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..2, 0..0);
    poll_until("initial hydration", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;
    let list_before = count_with_prefix(&log, "list");

    // Change the sort in place — NO new set_viewport. The machinery must rebuild
    // the ordered index for the new variant (one list page) and restart the
    // detail pass on its own. Before the fix, set_order fell to the single-pass
    // reseed-from-cache path: it never listed the new variant nor re-issued the
    // viewport, so `list` stayed flat and augmentation was dead.
    scenery.set_order(Some(("branch".to_string(), SortDirection::Ascending)));

    // The fixture's branches are r0=main, r1=dev, so ascending by `branch` puts
    // r1 first. Wait on the resulting *order*, not merely on "row 0 is Fresh":
    // the pre-resort map already satisfies the latter, so a status-only wait can
    // be satisfied before the reorder has landed. (That race is why this test
    // asserted the unsorted `full-r0` and still passed on some machines, back
    // when `resort` dropped the sort entirely.)
    poll_until("new sort variant is listed and applied", || {
        count_with_prefix(&log, "list") > list_before
            && detail_of(&scenery, 0).as_deref() == Some("full-r1")
    })
    .await;

    assert!(
        count_with_prefix(&log, "list") > list_before,
        "a sort change must list the new variant, not silently reseed from cache"
    );
    // Soft-refresh: the visible row never blanks to None, and its augmented
    // (detail) columns survive the reorder because the cache is keyed by id.
    assert!(
        scenery.row(0).is_some(),
        "row 0 stays present across the resort"
    );
    assert_eq!(
        detail_of(&scenery, 0).as_deref(),
        Some("full-r1"),
        "ascending by branch puts dev (r1) above main (r0)"
    );
    // The sorted view shows r1 and r3 (both dev) in the viewport 0..2. r3 has
    // never hydrated, so the restarted detail pass fetches it: the rows on
    // screen, not whichever ids sit at those positions in the index. Let that
    // land before counting, so the revert below is measured on its own.
    poll_until("the sorted view hydrates the rows it shows", || {
        matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
            && detail_of(&scenery, 1).as_deref() == Some("full-r3")
    })
    .await;

    // Reverting to a variant already listed reorders straight from cache — its
    // index exists and its rows are `Complete`, so neither a list nor a detail
    // call is issued, and the grid never blanks.
    let list_after_sort = count_with_prefix(&log, "list");
    let detail_after_sort = count_with_prefix(&log, "detail");
    scenery.set_order(None);
    // Back to the index's own order, so r0 leads again — waiting on the order
    // rather than the status, for the same reason as above.
    poll_until("revert reseeds from cache", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && detail_of(&scenery, 0).as_deref() == Some("full-r0")
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

/// A detail-pass failure for one id marks only that row failed; the others
/// hydrate normally.
#[tokio::test]
async fn detail_failure_marks_only_that_row() {
    let tmp = TempDir::new().unwrap();
    let log = tmp.path().join("calls.log");

    // Bake FAIL_ID into the detail cmd so r1 errors.
    let detail_cmd = make_cmd(&log)
        .with_env("FAIL_ID", "r1")
        .with_table(ENTITY, CmdSpec::new(LIST).with_detail(DETAIL));
    let lens = two_pass_lens(tmp.path());
    let dio = two_pass_dio(&lens, &make_cmd(&log), &detail_cmd).await;
    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..2, 0..0);

    poll_until("r0 fresh, r1 failed", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::LoadFailed { .. }))
    })
    .await;

    assert_eq!(detail_of(&scenery, 0).as_deref(), Some("full-r0"));
    // The failed row keeps its cheap list column and is not Fresh.
    assert_eq!(branch_of(&scenery, 1).as_deref(), Some("dev"));
}
