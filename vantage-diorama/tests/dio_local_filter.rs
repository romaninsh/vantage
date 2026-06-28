//! Stage 3: local emulation. A condition on an **augmented** column — one the
//! master can't filter because it's produced client-side — is applied locally
//! over the hydrated cache. The match is unknowable until a row is augmented, so
//! matching rows surface as they hydrate ("fetch all → augment → keep applying
//! the filter").

mod support;

use support::{MockView, bucket_dio};

#[tokio::test]
async fn condition_on_augmented_column_filters_locally() {
    // `name` is augmented from the `names` source; 1st and 3rd objects are John.
    let dio = bucket_dio().await;
    dio.with_condition_eq("name", "John");

    let view = MockView::open(&dio, 10).await;
    view.viewport(0..10); // pull the whole set into view so every row hydrates

    view.settle_until("filtered to the two Johns", |v| v.loaded_rows() == 2)
        .await;

    assert_eq!(view.loaded_rows(), 2, "only the two John rows remain");
    assert_eq!(view.row_count(), 2);
    assert_eq!(view.col_at(0, "name").as_deref(), Some("John"));
    assert_eq!(view.col_at(1, "name").as_deref(), Some("John"));
}

#[tokio::test]
async fn no_condition_keeps_all_augmented_rows() {
    let dio = bucket_dio().await;
    let view = MockView::open(&dio, 10).await;
    view.viewport(0..10);
    view.settle_until("all hydrated", |v| v.loaded_rows() == 3).await;
    assert_eq!(view.loaded_rows(), 3);
}

#[tokio::test]
async fn sort_on_augmented_column_orders_locally() {
    // The master can't sort by `name` (it's augmented). Sort is emulated over
    // the hydrated cache. Names: o1=John, o2=Jane, o3=John → asc: Jane, John, John.
    let dio = bucket_dio().await;
    dio.with_order("name", vantage_diorama::SortDir::Asc);

    let view = MockView::open(&dio, 10).await;
    view.viewport(0..10);
    view.settle_until("sorted by name", |v| v.loaded_rows() == 3)
        .await;

    assert_eq!(view.col_at(0, "name").as_deref(), Some("Jane"));
    assert_eq!(view.col_at(1, "name").as_deref(), Some("John"));
    assert_eq!(view.col_at(2, "name").as_deref(), Some("John"));
}
