//! A row whose detail fetch failed carries no augmented value, so a view
//! judging one never shows it, either way the clause points, and reads
//! `Partial` while it leaves the row out. And a lift-all augmentation, which
//! declares no columns, makes every column one that may be augmented.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Clause, LoadState, TableScenery};
use vantage_vista::{FilterOp, Vista};

use super::augmented_views::{name_of, settle, settle_as};
use super::big_bucket::{DetailProbe, john_ids, master_shell, probed_dio};
use crate::support::{cond, wait_listed};

const FAILING: &str = "o017";

/// `others` never shows `FAILING`, a John whose detail fetch fails.
fn never_failing(scenery: &Arc<dyn TableScenery>) {
    let shown = super::fixtures::ids(scenery);
    assert!(!shown.iter().any(|id| id == FAILING), "{FAILING} shown");
}

async fn failing_dio() -> vantage_diorama::Dio {
    let master = Vista::new("bucket", master_shell());
    probed_dio(master, DetailProbe::failing(FAILING), &["name", "size"]).await
}

#[tokio::test]
async fn an_eq_view_leaves_a_failed_row_out_and_reads_partial() -> Result<()> {
    let johns = failing_dio()
        .await
        .with_query(&cond("name", FilterOp::Eq, "John"))
        .await?;
    let view = johns.table_scenery().page_size(20).open().await?;
    view.set_viewport(0..20);
    let settled = settle_as(&view, LoadState::Partial, never_failing).await;
    let want: Vec<String> = john_ids().into_iter().filter(|id| id != FAILING).collect();
    assert_eq!(settled, want);
    Ok(())
}

#[tokio::test]
async fn a_ne_view_never_admits_a_failed_row_and_reads_partial() -> Result<()> {
    let others = failing_dio()
        .await
        .with_query(&cond("name", FilterOp::Ne, "John"))
        .await?;
    let view = others.table_scenery().page_size(20).open().await?;
    view.set_viewport(0..20);
    let settled = settle_as(&view, LoadState::Partial, never_failing).await;
    assert_eq!(settled.len(), 180, "every Jane; the failed John isn't one");
    Ok(())
}

async fn lift_all_dio() -> vantage_diorama::Dio {
    probed_dio(
        Vista::new("bucket", master_shell()),
        DetailProbe::default(),
        &[],
    )
    .await
}

/// Any column may be lifted from the detail, `kind` included, so a picker
/// that never hydrates can't judge a condition on it.
#[tokio::test]
async fn a_lift_all_picker_with_a_condition_fails() -> Result<()> {
    let dio = lift_all_dio().await;
    let view = dio
        .table_scenery()
        .titles_only()
        .where_eq("kind", "red")
        .open()
        .await?;
    wait_listed(&view).await;
    match view.load_state() {
        LoadState::Failed(failure) => {
            assert_eq!(
                failure.clause,
                Some(Clause::Filter("kind".into())),
                "{failure}"
            );
        }
        other => panic!("expected a loud failure, got {other:?}"),
    }
    Ok(())
}

/// A refined view over a lift-all Dio sweeps at open, with no viewport, and
/// shows only hydrated rows.
#[tokio::test]
async fn a_lift_all_refined_view_sweeps_before_it_settles() -> Result<()> {
    // Held: a view keeps only a weak handle on its Dio.
    let dio = lift_all_dio().await;
    let view = dio
        .table_scenery()
        .page_size(20)
        .where_eq("kind", "red")
        .open()
        .await?;
    let settled = settle(&view, |scenery| {
        for i in 0..scenery.row_count() {
            assert!(
                matches!(name_of(scenery, i), Some(CborValue::Text(_))),
                "row {i} shown before it hydrated"
            );
        }
    })
    .await;
    assert_eq!(settled.len(), 100, "every red object");
    Ok(())
}
