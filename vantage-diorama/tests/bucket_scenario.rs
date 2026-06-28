//! Stage 0 proof: a bucket master augmented with per-object detail loads
//! cache-first and stale-while-refresh — the view shows GRAY (known-but-
//! unhydrated) rows immediately, then they hydrate to LOADED as the detail
//! pass runs. Proves the `MockView` + `cache_in_memory()` harness end to end.

mod support;

use support::{MockView, bucket_dio};

#[tokio::test]
async fn grid_shows_gray_rows_then_hydrates() {
    let dio = bucket_dio().await;
    let view = MockView::open(&dio, 3).await;

    // List pass ran on open: 3 rows known to exist, none hydrated yet.
    assert_eq!(view.gray_rows(), 3, "all three objects listed as gray");
    assert_eq!(
        view.loaded_rows(),
        0,
        "nothing hydrated before the viewport"
    );
    assert!(view.is_loading());

    // A grid scrolls them into view → the detail pass hydrates each row.
    view.viewport(0..3);
    view.settle_until("all rows hydrated", |v| v.loaded_rows() == 3)
        .await;

    assert_eq!(view.gray_rows(), 0);
    assert_eq!(view.loaded_rows(), 3);
    assert!(!view.is_loading());
}
