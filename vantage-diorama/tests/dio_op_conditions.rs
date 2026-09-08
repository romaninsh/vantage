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

/// Runtime terms — the grid's filter panel — narrow an already-open view
/// and clear back to the full set, and the view reports what is in force.
#[tokio::test]
async fn runtime_filter_terms_narrow_and_clear() {
    use vantage_diorama::OpCondition;

    let dio: Dio = eager_dio(teams_master()).await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("all rows", |v| v.row_count() == 3).await;

    view.scenery()
        .set_filter_terms(vec![OpCondition::new("team", FilterOp::Ne, "red")]);
    view.settle_until("only non-red rows", |v| v.row_count() == 1)
        .await;
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
    assert_eq!(view.scenery().filter_terms().len(), 1);

    // A pattern match is evaluated locally: this master pushes nothing.
    view.scenery()
        .set_filter_terms(vec![OpCondition::new("team", FilterOp::Like, "RE")]);
    view.settle_until("case-insensitive substring", |v| v.row_count() == 2)
        .await;

    view.scenery().set_filter_terms(Vec::new());
    view.settle_until("back to all rows", |v| v.row_count() == 3)
        .await;
    assert!(view.scenery().filter_terms().is_empty());
}
