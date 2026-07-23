//! Local operator filters. A scenery `where_op(col, op, value)` narrows the
//! visible set over the cache using a comparison operator (`!=`, `in`, …) — the
//! fallback the Dio applies when the master vista can't push the operator into
//! its query. The `MockShell` master here advertises no operator push-down
//! (`can_filter_operators = false` by default), so every operator resolves
//! locally, exactly like a REST/CSV-backed table.
//!
//! `teams_master`: rows a=red, b=blue, c=red on a native `team` column.

mod support;

use ciborium::Value as CborValue;
use support::{MockView, eager_dio, teams_master};
use vantage_diorama::Dio;
use vantage_vista::FilterOp;

#[tokio::test]
async fn ne_filters_out_the_equal_rows() {
    let dio: Dio = eager_dio(teams_master()).await;
    let view = MockView::open_with(&dio, 10, |b| b.where_op("team", FilterOp::Ne, "red")).await;
    view.settle_until("only non-red rows", |v| v.row_count() == 1)
        .await;

    assert_eq!(
        view.row_count(),
        1,
        "only the one blue row survives `!= red`"
    );
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
}

#[tokio::test]
async fn in_set_keeps_only_listed_values() {
    let dio: Dio = eager_dio(teams_master()).await;
    let set = CborValue::Array(vec![CborValue::Text("blue".into())]);
    let view =
        MockView::open_with(&dio, 10, move |b| b.where_op("team", FilterOp::InSet, set)).await;
    view.settle_until("only blue", |v| v.row_count() == 1).await;

    assert_eq!(view.row_count(), 1);
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
}

#[tokio::test]
async fn not_in_set_excludes_listed_values() {
    let dio: Dio = eager_dio(teams_master()).await;
    let set = CborValue::Array(vec![CborValue::Text("red".into())]);
    let view = MockView::open_with(&dio, 10, move |b| {
        b.where_op("team", FilterOp::NotInSet, set)
    })
    .await;
    view.settle_until("everything but red", |v| v.row_count() == 1)
        .await;

    assert_eq!(view.row_count(), 1, "both reds excluded, blue remains");
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
}

#[tokio::test]
async fn no_op_condition_keeps_all_rows() {
    let dio: Dio = eager_dio(teams_master()).await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("all rows", |v| v.row_count() == 3).await;
    assert_eq!(view.row_count(), 3);
}
