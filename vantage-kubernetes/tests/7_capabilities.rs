//! Live tests for the Vista read capabilities: ordering, quicksearch, and
//! pagination — all honoured client-side over the materialised listing.
//! Requires a seeded cluster and `RUN_K8S_INTEGRATION=1`.

mod common;

use common::{as_text, cluster_or_skip, NS};
use vantage_kubernetes::models::core::pods;
use vantage_vista::SortDirection;

#[tokio::test]
async fn ordering_is_applied() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let mut vista = cluster.vista_factory().from_table(pods::pods_table(cluster.clone()))?;
    vista.add_condition_eq("namespace", ciborium::Value::Text(NS.to_string()))?;

    vista.add_order("name", SortDirection::Ascending)?;
    let asc: Vec<String> = vista
        .fetch_window(0, 1000)
        .await?
        .iter()
        .filter_map(|(_, r)| as_text(r.get("name")))
        .collect();
    let mut expected = asc.clone();
    expected.sort();
    assert_eq!(asc, expected, "ascending order should be sorted by name");

    vista.add_order("name", SortDirection::Descending)?;
    let desc: Vec<String> = vista
        .fetch_window(0, 1000)
        .await?
        .iter()
        .filter_map(|(_, r)| as_text(r.get("name")))
        .collect();
    let mut expected_desc = asc.clone();
    expected_desc.sort_by(|a, b| b.cmp(a));
    assert_eq!(desc, expected_desc, "descending order should be reverse-sorted");
    Ok(())
}

#[tokio::test]
async fn quicksearch_narrows_to_matching_rows() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let mut vista = cluster.vista_factory().from_table(pods::pods_table(cluster.clone()))?;
    vista.add_condition_eq("namespace", ciborium::Value::Text(NS.to_string()))?;
    vista.add_search("sidecar")?;

    let rows = vista.fetch_window(0, 1000).await?;
    assert_eq!(rows.len(), 1, "search 'sidecar' should match exactly one pod");
    assert_eq!(as_text(rows[0].1.get("name")).as_deref(), Some("sidecar"));

    // Count honours the search filter too.
    assert_eq!(vista.get_count().await?, 1);

    vista.clear_search()?;
    assert!(vista.get_count().await? > 1, "clearing search should restore the full count");
    Ok(())
}

#[tokio::test]
async fn pagination_slices_the_listing() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let mut vista = cluster.vista_factory().from_table(pods::pods_table(cluster.clone()))?;
    vista.add_condition_eq("namespace", ciborium::Value::Text(NS.to_string()))?;
    vista.add_order("name", SortDirection::Ascending)?;

    let total = vista.get_count().await? as usize;
    assert!(total >= 4, "expected ≥4 demo pods for a meaningful page test");

    vista.set_page_size(2)?;
    let page1 = vista.fetch_page(1).await?;
    let page2 = vista.fetch_page(2).await?;
    assert_eq!(page1.len(), 2, "page 1 should hold the page size");
    assert_eq!(page2.len(), 2, "page 2 should hold the page size");
    // Pages don't overlap and follow the global sort order.
    assert_ne!(page1[0].0, page2[0].0, "pages should be disjoint");

    let window = vista.fetch_window(0, 2).await?;
    assert_eq!(
        window.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        page1.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        "window [0,2) should equal page 1"
    );
    Ok(())
}
