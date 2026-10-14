//! What views demand drives the detail pass, and a UI filter narrows on a
//! column that only the augmentation supplies.

use std::time::Duration;

use tempfile::TempDir;
use vantage_dataset::prelude::WritableValueSet;
use vantage_diorama::{RowStatus, Source};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Condition, FilterOp, Vista};

use crate::support::{self, meta, poll_until, record, status_of, text};
use crate::{aug, col_by_id, col_of, folder_size_dio, open};

/// DEMAND gates the detail pass: a view demanding only cheap columns never
/// pays for the augment; opening a view that demands an augment column starts
/// the fetches (gap rows only); closing it stops them — a later demotion
/// leaves the augment blank until something demands it again.
#[tokio::test]
async fn augment_fetches_only_while_a_view_demands_augment_columns() {
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
    tree.set_viewport(0..2, 0..0);
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
    listing.set_viewport(0..2, 0..0);
    poll_until("augment lands once demanded", || {
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
    tree.set_viewport(0..2, 0..0);
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
    listing.set_viewport(0..2, 0..0);
    // The stale value already reads "4096", so the wait must be on the FETCH
    // COUNTER — the visible value merely stops being stale when it lands.
    poll_until("re-demanded row refetches", || gets() == 3).await;
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

    scenery.set_viewport(0..2, 0..0);
    poll_until("rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;
    assert_eq!(scenery.row_count(), 2);

    scenery.set_filters(vec![Condition::new(
        "detail",
        FilterOp::Eq,
        text("full-r0"),
    )]);
    poll_until("filter narrowed the visible set", || {
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
    poll_until("clearing restores the full set", || {
        scenery.row_count() == 2
    })
    .await;
}
