//! Live `set_sort` / `set_search` on a two-pass (augmented) scenery.
//!
//! The builder's `.sort(..)` path is covered elsewhere; what these cover is
//! changing the order on an already-open handle, which is what a grid header
//! click does.

mod support;

use support::{MockView, bucket_dio};
use vantage_diorama::SortDir;

/// Ids in display order.
fn order(view: &MockView) -> Vec<String> {
    (0..view.row_count())
        .filter_map(|i| view.col_at(i, "id"))
        .collect()
}

/// A grid opened with no sort, then sorted by a header click.
///
/// `kind` is a native list-pass column, present on the cheap rows from the
/// moment they land — nothing here needs hydration to be sortable.
#[tokio::test]
async fn set_sort_orders_a_scenery_opened_without_one() {
    let dio = bucket_dio().await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("rows listed", |v| v.row_count() == 3)
        .await;
    assert_eq!(order(&view), vec!["o1", "o2", "o3"], "native list order");

    // red, red, blue → o1, o3, o2. Different from native, so the assertion
    // can't pass by accident.
    view.scenery()
        .set_sort(Some("kind".to_string()), SortDir::Desc);
    view.settle_until("sorted by kind desc", |v| {
        order(v) == vec!["o1", "o3", "o2"]
    })
    .await;
}

/// Changing the sort on a handle that already had one.
#[tokio::test]
async fn set_sort_replaces_an_existing_order() {
    let dio = bucket_dio().await;
    let view = MockView::open_with(&dio, 10, |b| b.sort("kind", SortDir::Asc)).await;
    view.settle_until("sorted by kind asc", |v| order(v) == vec!["o2", "o1", "o3"])
        .await;

    view.scenery()
        .set_sort(Some("kind".to_string()), SortDir::Desc);
    view.settle_until("re-sorted by kind desc", |v| {
        order(v) == vec!["o1", "o3", "o2"]
    })
    .await;
}

/// Sorting by an augmented column. The values arrive with hydration, so the
/// order settles once the rows are complete.
#[tokio::test]
async fn set_sort_orders_by_an_augmented_column() {
    let dio = bucket_dio().await;
    let view = MockView::open(&dio, 10).await;
    view.viewport(0..10);
    view.settle_until("hydrated", |v| v.loaded_rows() == 3)
        .await;

    // Jane, John, John → o2 first.
    view.scenery()
        .set_sort(Some("name".to_string()), SortDir::Asc);
    view.settle_until("sorted by name asc", |v| order(v) == vec!["o2", "o1", "o3"])
        .await;
}

/// Clearing the sort returns the view to the index's own order.
#[tokio::test]
async fn clearing_the_sort_restores_list_order() {
    let dio = bucket_dio().await;
    let view = MockView::open_with(&dio, 10, |b| b.sort("kind", SortDir::Desc)).await;
    view.settle_until("sorted", |v| order(v) == vec!["o1", "o3", "o2"])
        .await;

    view.scenery().set_sort(None, SortDir::Asc);
    view.settle_until("back to list order", |v| order(v) == vec!["o1", "o2", "o3"])
        .await;
}

/// `set_search` on a handle opened without one narrows the visible set.
#[tokio::test]
async fn set_search_narrows_a_scenery_opened_without_one() {
    let dio = bucket_dio().await;
    let view = MockView::open(&dio, 10).await;
    view.viewport(0..10);
    view.settle_until("hydrated", |v| v.loaded_rows() == 3)
        .await;

    view.scenery().set_search(Some("blue".to_string()));
    view.settle_until("narrowed to the blue row", |v| order(v) == vec!["o2"])
        .await;

    view.scenery().set_search(None);
    view.settle_until("widened again", |v| v.row_count() == 3)
        .await;
}
