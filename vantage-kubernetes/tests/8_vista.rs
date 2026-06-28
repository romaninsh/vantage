//! Live Vista-contract tests: list / count / get / capabilities round-trip
//! through the type-erased boundary. Requires a seeded cluster and
//! `RUN_K8S_INTEGRATION=1`.

mod common;

use common::cluster_or_skip;
use vantage_kubernetes::models::core::pods;

#[tokio::test]
async fn vista_lists_counts_and_advertises_capabilities() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let vista = cluster.vista_factory().from_table(pods::pods_table(cluster.clone()))?;

    // Read-only backend: count is supported, writes are not.
    assert!(vista.capabilities().can_count, "pods vista should advertise can_count");

    let count = vista.get_count().await?;
    let rows = vista.fetch_window(0, 10000).await?;
    assert_eq!(count as usize, rows.len(), "count should match the listing length");

    // Schema is exposed through the erased boundary.
    assert_eq!(vista.get_id_column(), Some("id"));
    let columns = vista.get_column_names();
    for expected in ["name", "namespace", "phase", "ready"] {
        assert!(columns.contains(&expected), "pods vista should expose `{expected}`");
    }

    // Every listed row carries a non-empty id (the IndexMap key).
    for (id, _) in &rows {
        assert!(!id.is_empty(), "every pod row should have an id");
    }
    Ok(())
}
