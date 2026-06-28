//! Stage 1a: the **Dio** owns conditions. A condition set on the Dio is
//! inherited by every scenery opened on it — the Dio defines "what this table
//! is", the view just observes it. For a native column on a single-pass Dio the
//! filter resolves locally over the cache.

mod support;

use support::{MockView, eager_dio, teams_master};
use vantage_diorama::{Dio, SortDir};

#[tokio::test]
async fn dio_condition_is_inherited_by_every_view() {
    let dio: Dio = eager_dio(teams_master()).await;
    dio.with_condition_eq("team", "red");

    let view = MockView::open(&dio, 10).await;
    view.settle_until("filtered to red", |v| v.loaded_rows() == 2)
        .await;

    assert_eq!(view.loaded_rows(), 2, "only the two red rows show");
    assert_eq!(view.row_count(), 2);
    assert!(!view.is_loading());
}

#[tokio::test]
async fn no_condition_shows_all_rows() {
    let dio: Dio = eager_dio(teams_master()).await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("all rows", |v| v.loaded_rows() == 3)
        .await;
    assert_eq!(view.loaded_rows(), 3);
}

#[tokio::test]
async fn dio_order_is_inherited_by_every_view() {
    let dio: Dio = eager_dio(teams_master()).await;
    dio.with_order("team", SortDir::Asc);

    let view = MockView::open(&dio, 10).await;
    view.settle_until("sorted", |v| v.loaded_rows() == 3).await;

    // Ascending by team: blue sorts before the two reds.
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
    assert_eq!(view.col_at(1, "team").as_deref(), Some("red"));
    assert_eq!(view.col_at(2, "team").as_deref(), Some("red"));
}
