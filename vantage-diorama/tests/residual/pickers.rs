//! A titles-only picker over a two-pass Dio lists cheap rows and never
//! hydrates. It still evaluates its clauses and the Dio's residual over the
//! listed index, and fails on a clause naming an augmented column, which
//! it could never match.

use vantage_core::Result;
use vantage_diorama::{Clause, Dio, LoadState, TableScenery};
use vantage_vista::{Condition, FilterOp, SortDirection};

use super::fixtures::wait_for_ids;
use crate::support::{bucket_dio, wait_listed};

async fn picker(dio: &Dio) -> std::sync::Arc<dyn TableScenery> {
    let view = dio
        .table_scenery()
        .titles_only()
        .page_size(10)
        .open()
        .await
        .expect("picker opens");
    wait_listed(&view).await;
    view
}

#[tokio::test]
async fn a_picker_applies_its_search_over_the_listed_rows() -> Result<()> {
    let dio = bucket_dio().await;
    let view = picker(&dio).await;
    wait_for_ids(&view, &["o1", "o2", "o3"]).await;
    view.set_search(Some("red".into()));
    wait_for_ids(&view, &["o1", "o3"]).await;
    Ok(())
}

/// The picker has failed on `clause` over the augmented `name`, and the list
/// pass, landing later, put no rows in it.
async fn assert_fails_on_name(view: &std::sync::Arc<dyn TableScenery>, clause: Clause) {
    match view.load_state() {
        LoadState::Failed(failure) => {
            assert_eq!(failure.clause, Some(clause), "{failure}");
        }
        other => panic!("expected a loud failure, got {other:?}"),
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(view.row_count(), 0, "no rows in a failed picker");
    assert!(view.row(0).is_none(), "no rows in a failed picker");
}

#[tokio::test]
async fn a_picker_filtered_on_an_augmented_column_fails() -> Result<()> {
    let dio = bucket_dio().await;
    let view = dio
        .table_scenery()
        .titles_only()
        .filter(Condition::new("name", FilterOp::Eq, "John"))
        .open()
        .await?;
    wait_listed(&view).await;
    assert_fails_on_name(&view, Clause::Filter("name".into())).await;
    Ok(())
}

/// The Dio's residual names the augmented column, not the picker's query.
#[tokio::test]
async fn a_picker_over_a_dio_narrowed_on_an_augmented_column_fails() -> Result<()> {
    let johns = bucket_dio()
        .await
        .derive()
        .with_condition(Condition::new("name", FilterOp::Eq, "John"))
        .build()
        .await?;
    assert_eq!(johns.residual().conditions.len(), 1, "augmented: residual");
    let view = johns.table_scenery().titles_only().open().await?;
    wait_listed(&view).await;
    assert_fails_on_name(&view, Clause::Filter("name".into())).await;
    Ok(())
}

#[tokio::test]
async fn a_picker_sorted_by_an_augmented_column_fails() -> Result<()> {
    let dio = bucket_dio().await;
    let view = picker(&dio).await;
    view.set_order(Some(("name".into(), SortDirection::Ascending)));
    assert_fails_on_name(&view, Clause::Order("name".into())).await;
    Ok(())
}
