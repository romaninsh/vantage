//! Live tests for the Vista read capabilities: ordering, quicksearch, and
//! windows — all honoured client-side over the listed collection.
//! Requires a seeded cluster and `RUN_K8S_INTEGRATION=1`.

mod common;

use common::{NS, as_text, cluster_or_skip};
use vantage_kubernetes::models::core::pods;
use vantage_vista::{FilterOp, SortDirection, Vista};

async fn demo_pods() -> anyhow::Result<Option<Vista>> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(None);
    };
    let vista = cluster
        .vista_factory()
        .from_table(pods::pods_table(cluster.clone()))?
        .with_condition("namespace", FilterOp::Eq, NS)?;
    Ok(Some(vista))
}

async fn names(vista: &Vista) -> anyhow::Result<Vec<String>> {
    Ok(vista
        .fetch_window(0, 1000)
        .await?
        .rows
        .iter()
        .filter_map(|(_, r)| as_text(r.get("name")))
        .collect())
}

#[tokio::test]
async fn ordering_is_applied() -> anyhow::Result<()> {
    let Some(mut vista) = demo_pods().await? else {
        return Ok(());
    };

    vista.set_order(Some(("name", SortDirection::Ascending)))?;
    let asc = names(&vista).await?;
    let mut expected = asc.clone();
    expected.sort();
    assert_eq!(asc, expected, "ascending order should be sorted by name");

    vista.set_order(Some(("name", SortDirection::Descending)))?;
    let desc = names(&vista).await?;
    let mut expected_desc = asc.clone();
    expected_desc.sort_by(|a, b| b.cmp(a));
    assert_eq!(
        desc, expected_desc,
        "descending order should be reverse-sorted"
    );
    Ok(())
}

#[tokio::test]
async fn quicksearch_narrows_to_matching_rows() -> anyhow::Result<()> {
    let Some(mut vista) = demo_pods().await? else {
        return Ok(());
    };
    vista.set_search(Some("sidecar"))?;

    let rows = vista.fetch_window(0, 1000).await?.rows;
    assert_eq!(
        rows.len(),
        1,
        "search 'sidecar' should match exactly one pod"
    );
    assert_eq!(as_text(rows[0].1.get("name")).as_deref(), Some("sidecar"));

    // Count honours the search filter too.
    assert_eq!(vista.count().await?, 1);

    vista.set_search(None)?;
    assert!(
        vista.count().await? > 1,
        "clearing search should restore the full count"
    );
    Ok(())
}

#[tokio::test]
async fn windows_slice_the_listing() -> anyhow::Result<()> {
    let Some(vista) = demo_pods().await? else {
        return Ok(());
    };
    let vista = vista.with_order("name", SortDirection::Ascending)?;

    let total = vista.count().await? as usize;
    assert!(
        total >= 4,
        "expected ≥4 demo pods for a meaningful window test"
    );

    let first = vista.fetch_window(0, 2).await?;
    let second = vista.fetch_window(2, 2).await?;
    assert_eq!(first.rows.len(), 2, "a window holds its limit");
    assert_eq!(second.rows.len(), 2, "a window holds its limit");
    assert_eq!(
        first.total,
        Some(total as i64),
        "the page carries the total"
    );
    // Windows don't overlap and follow the global sort order.
    assert_ne!(
        first.rows[0].0, second.rows[0].0,
        "windows should be disjoint"
    );
    Ok(())
}
