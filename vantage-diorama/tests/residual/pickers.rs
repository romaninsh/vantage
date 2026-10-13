//! A titles-only picker over a two-pass Dio lists cheap rows and never
//! hydrates. It still evaluates its clauses and the Dio's residual over the
//! listed index, and fails on a clause naming an augmented column, which
//! it could never match.

use vantage_core::Result;
use vantage_diorama::{Clause, Dio, LoadState, TableScenery};
use vantage_vista::{Capability, FilterOp, Vista, VistaCapabilities, mocks::MockShell};

use super::fixtures::wait_for_ids;
use crate::support::{
    augment_name, augmented_dio, bucket_dio, cond, meta, quick_lens, record, wait_listed,
};

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

/// A bucket master that can't take a non-`Eq` operator, so a derived Dio's
/// `kind != blue` stays in its residual.
fn bucket_without_filter_ops() -> Vista {
    let caps: VistaCapabilities = [Capability::Window, Capability::Search, Capability::Order]
        .into_iter()
        .collect();
    let shell = MockShell::new()
        .with_record("o1", record(&[("id", "o1"), ("kind", "red")]))
        .with_record("o2", record(&[("id", "o2"), ("kind", "blue")]))
        .with_record("o3", record(&[("id", "o3"), ("kind", "red")]))
        .with_metadata(meta(&["id", "kind"]))
        .with_capabilities(caps);
    Vista::new("bucket", shell)
}

#[tokio::test]
async fn a_picker_lists_only_the_rows_the_dio_residual_admits() -> Result<()> {
    let parent = augmented_dio(
        &quick_lens(vantage_diorama::Cache::memory()),
        bucket_without_filter_ops(),
        vec![augment_name()],
    )
    .await;
    let reds = parent
        .with_query(&cond("kind", FilterOp::Ne, "blue"))
        .await?;
    assert_eq!(reds.residual().conditions.len(), 1, "the master refused it");
    let view = picker(&reds).await;
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
        .where_eq("name", "John")
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
        .with_query(&cond("name", FilterOp::Eq, "John"))
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
    view.set_sort(Some("name".into()), vantage_diorama::SortDir::Asc);
    assert_fails_on_name(&view, Clause::Order("name".into())).await;
    Ok(())
}
