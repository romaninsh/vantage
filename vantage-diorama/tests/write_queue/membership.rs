//! The membership events a two-pass view follows a confirmed write by.

use vantage_core::Result;

use crate::support::{MockView, bucket_dio, record};

/// Rows of `view` whose `kind` is `kind`.
fn count_kind(view: &MockView, kind: &str) -> usize {
    (0..view.row_count())
        .filter(|&i| view.col_at(i, "kind").as_deref() == Some(kind))
        .count()
}

/// A two-pass (augmented) view keeps its own index of ids, so it follows a
/// write only through membership events: a confirmed facade delete drops the
/// row, and a confirmed insert of a new id shows it.
#[tokio::test]
async fn a_two_pass_view_follows_facade_deletes_and_inserts() -> Result<()> {
    use vantage_dataset::traits::WritableValueSet;

    let dio = bucket_dio().await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("listed", |v| v.row_count() == 3).await;

    dio.cached().delete("o1").await?;
    view.settle_until("o1 dropped", |v| {
        v.row_count() == 2 && count_kind(v, "red") == 1
    })
    .await;

    dio.cached()
        .insert_value("o4", &record(&[("id", "o4"), ("kind", "green")]))
        .await?;
    view.settle_until("o4 shown", |v| {
        v.row_count() == 3 && count_kind(v, "green") == 1
    })
    .await;
    Ok(())
}
