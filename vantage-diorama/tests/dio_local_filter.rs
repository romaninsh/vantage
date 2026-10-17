//! Local emulation. A condition on an **augmented** column — one the master
//! can't filter because it's produced client-side — goes into the derived
//! Dio's residual and is applied locally over the hydrated cache. The match is
//! unknowable until a row is augmented, so matching rows surface as they
//! hydrate ("fetch all → augment → keep applying the filter").

mod support;

use support::{MockView, bucket_dio, cond};
use vantage_vista::{FilterOp, SortDirection};

#[tokio::test]
async fn native_column_condition_filters_cheap_rows_without_forcing_hydration() {
    // `kind` is a native list-pass column → the filter applies to the cheap
    // (gray) rows immediately and does NOT force full hydration. No viewport set.
    let dio = bucket_dio()
        .await
        .derive()
        .with_query(&cond("kind", FilterOp::Eq, "red"))
        .build()
        .await
        .expect("derives");

    let view = MockView::open(&dio, 10).await;
    view.settle_until("native filter pre-hydration", |v| v.row_count() == 2)
        .await;

    assert_eq!(view.row_count(), 2, "two red rows");
    assert_eq!(view.gray_rows(), 2, "shown as gray — no forced hydration");
    assert_eq!(view.loaded_rows(), 0);
}

#[tokio::test]
async fn sort_on_augmented_column_orders_locally() {
    // The master can't sort by `name` (it's augmented). Sort is emulated over
    // the hydrated cache. Names: o1=John, o2=Jane, o3=John → asc: Jane, John, John.
    let dio = bucket_dio()
        .await
        .derive()
        .with_order("name", SortDirection::Ascending)
        .build()
        .await
        .expect("derives");

    let view = MockView::open(&dio, 10).await;
    view.viewport(0..10);
    view.settle_until("sorted by name", |v| v.loaded_rows() == 3)
        .await;

    assert_eq!(view.col_at(0, "name").as_deref(), Some("Jane"));
    assert_eq!(view.col_at(1, "name").as_deref(), Some("John"));
    assert_eq!(view.col_at(2, "name").as_deref(), Some("John"));
}
