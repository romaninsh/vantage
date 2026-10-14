//! Local operator filters. A scenery `filter(Condition::new(col, op, value))`
//! narrows the visible set over the cache using a comparison operator (`!=`,
//! `in`, …) — the fallback the Dio applies when the master vista can't push
//! the operator into its query. The `MockShell` master here advertises no
//! operator push-down (`can_filter_operators = false` by default), so every
//! operator resolves locally, exactly like a REST/CSV-backed table.
//!
//! `teams_master`: rows a=red, b=blue, c=red on a native `team` column.

mod support;

use ciborium::Value as CborValue;
use support::{MockView, eager_dio, teams_master};
use vantage_diorama::Dio;
use vantage_vista::{Condition, FilterOp};

#[tokio::test]
async fn ne_filters_out_the_equal_rows() {
    let dio: Dio = eager_dio(teams_master()).await;
    let view = MockView::open_with(&dio, 10, |b| {
        b.filter(Condition::new("team", FilterOp::Ne, "red"))
    })
    .await;
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
    let view = MockView::open_with(&dio, 10, move |b| {
        b.filter(Condition::new("team", FilterOp::InSet, set))
    })
    .await;
    view.settle_until("only blue", |v| v.row_count() == 1).await;

    assert_eq!(view.row_count(), 1);
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
}

#[tokio::test]
async fn not_in_set_excludes_listed_values() {
    let dio: Dio = eager_dio(teams_master()).await;
    let set = CborValue::Array(vec![CborValue::Text("red".into())]);
    let view = MockView::open_with(&dio, 10, move |b| {
        b.filter(Condition::new("team", FilterOp::NotInSet, set))
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

/// Runtime filters — the grid's filter panel — narrow an already-open view
/// and clear back to the full set, and the view reports what is in force.
#[tokio::test]
async fn runtime_filters_narrow_and_clear() {
    let dio: Dio = eager_dio(teams_master()).await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("all rows", |v| v.row_count() == 3).await;

    view.scenery().set_filters(vec![Condition::new("team", FilterOp::Ne, "red")]);
    view.settle_until("only non-red rows", |v| v.row_count() == 1)
        .await;
    assert_eq!(view.col_at(0, "team").as_deref(), Some("blue"));
    assert_eq!(view.scenery().view_stats().filters.len(), 1);

    // A pattern match is evaluated locally: this master pushes nothing.
    view.scenery().set_filters(vec![Condition::new("team", FilterOp::Like, "RE")]);
    view.settle_until("case-insensitive substring", |v| v.row_count() == 2)
        .await;

    view.scenery().set_filters(Vec::new());
    view.settle_until("back to all rows", |v| v.row_count() == 3)
        .await;
    assert!(view.scenery().view_stats().filters.is_empty());
}

/// A datetime operand (`Tag(0, rfc3339)`, what a script's `days_ago(..)`
/// sends) orders against dates a source stores as text.
#[tokio::test]
async fn datetime_operand_orders_against_text_dates() {
    use support::{meta, record};
    use vantage_vista::{Vista, mocks::MockShell};

    let shell = MockShell::new()
        .with_record(
            "old",
            record(&[("id", "old"), ("updated_at", "2026-10-01T00:00:00Z")]),
        )
        .with_record(
            "new",
            record(&[("id", "new"), ("updated_at", "2026-10-09T00:00:00Z")]),
        )
        .with_metadata(meta(&["id", "updated_at"]));
    let dio: Dio = eager_dio(Vista::new("events", shell)).await;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("all rows", |v| v.row_count() == 2).await;

    let since = CborValue::Tag(0, Box::new(CborValue::Text("2026-10-05T00:00:00Z".into())));
    let after = vec![Condition::new("updated_at", FilterOp::Gt, since)];
    assert_eq!(
        view.scenery().count_where(after.clone()).await.unwrap(),
        Some(1)
    );
    view.scenery().set_filters(after);
    view.settle_until("only the later row", |v| v.row_count() == 1)
        .await;
    assert_eq!(view.col_at(0, "id").as_deref(), Some("new"));
}
