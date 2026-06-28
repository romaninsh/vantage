//! Live read tests: list resources and check the projected columns.
//! Requires a seeded cluster and `RUN_K8S_INTEGRATION=1`.

mod common;

use ciborium::Value as CborValue;
use common::{as_text, cluster_or_skip, NS};
use vantage_kubernetes::models::core::{namespaces, nodes, pods};

#[tokio::test]
async fn nodes_have_numeric_capacity() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let vista = cluster.vista_factory().from_table(nodes::nodes_table(cluster.clone()))?;
    let rows = vista.fetch_window(0, 1000).await?;

    assert!(!rows.is_empty(), "cluster should report at least one node");
    let (_, node) = &rows[0];
    // The projector parsed `status.capacity.cpu` into millicores — proving
    // quantity parsing works (a raw chart would have dropped the string).
    match node.get("cpuCapacityMillicores") {
        Some(CborValue::Integer(n)) => assert!(i128::from(*n) > 0, "cpu capacity should be positive"),
        other => panic!("expected integer cpuCapacityMillicores, got {other:?}"),
    }
    assert!(as_text(node.get("name")).is_some(), "node should have a name");
    Ok(())
}

#[tokio::test]
async fn namespaces_include_demo_and_kube_system() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let vista = cluster
        .vista_factory()
        .from_table(namespaces::namespaces_table(cluster.clone()))?;
    let rows = vista.fetch_window(0, 1000).await?;
    let names: Vec<String> = rows.iter().filter_map(|(_, r)| as_text(r.get("name"))).collect();

    assert!(names.iter().any(|n| n == NS), "expected the `{NS}` namespace; got {names:?}");
    assert!(names.iter().any(|n| n == "kube-system"), "expected kube-system");
    Ok(())
}

#[tokio::test]
async fn demo_pods_are_listed_with_columns() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let mut vista = cluster.vista_factory().from_table(pods::pods_table(cluster.clone()))?;
    vista.add_condition_eq("namespace", CborValue::Text(NS.to_string()))?;
    let rows = vista.fetch_window(0, 1000).await?;

    // 3 web replicas + the sidecar pod (+ maybe a completed job pod).
    assert!(rows.len() >= 4, "expected ≥4 pods in `{NS}`, got {}", rows.len());

    // Every pod is in `demo` (the namespace filter is real) and carries a
    // projected `ready` column like "1/1".
    for (_, pod) in &rows {
        assert_eq!(as_text(pod.get("namespace")).as_deref(), Some(NS));
        assert!(as_text(pod.get("ready")).is_some(), "pod should have a ready column");
    }

    // The two-container sidecar pod projects "2/2".
    let sidecar = rows.iter().find(|(_, r)| as_text(r.get("name")).as_deref() == Some("sidecar"));
    if let Some((_, pod)) = sidecar {
        assert_eq!(as_text(pod.get("ready")).as_deref(), Some("2/2"));
    }
    Ok(())
}
