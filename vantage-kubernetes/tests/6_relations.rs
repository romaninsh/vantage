//! Live relation tests: the headline `deployment → pods` drill plus
//! `node → pods` narrowing. Requires a seeded cluster and
//! `RUN_K8S_INTEGRATION=1`.

mod common;

use ciborium::Value as CborValue;
use common::{as_text, cluster_or_skip, NS, WEB_REPLICAS};
use vantage_kubernetes::models::apps::deployments;
use vantage_kubernetes::models::core::{nodes, pods};

#[tokio::test]
async fn deployment_web_drills_to_its_pods() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };

    // Grab the `web` deployment row (its id is the deployment name).
    let mut deployments = cluster
        .vista_factory()
        .from_table(deployments::deployments_table(cluster.clone()))?;
    deployments.add_condition_eq("id", CborValue::Text("web".to_string()))?;
    let rows = deployments.fetch_window(0, 10).await?;
    let (_, web) = rows
        .into_iter()
        .find(|(_, r)| as_text(r.get("namespace")).as_deref() == Some(NS))
        .expect("the `web` deployment should exist in `demo`");

    // Drill into its pods. The join is pod.ownerDeployment == web.id, so it
    // must return exactly the replica count — not every pod in the cluster.
    let pods_of_web = deployments.get_ref("pods", &web)?;
    let pod_rows = pods_of_web.fetch_window(0, 1000).await?;
    assert_eq!(
        pod_rows.len(),
        WEB_REPLICAS,
        "deployment web should own exactly {WEB_REPLICAS} pods, got {}",
        pod_rows.len()
    );
    for (_, pod) in &pod_rows {
        assert_eq!(as_text(pod.get("app")).as_deref(), Some("web"));
    }

    // And it should have at least one ReplicaSet.
    let rs = deployments.get_ref("replicasets", &web)?;
    assert!(!rs.fetch_window(0, 10).await?.is_empty(), "web should have a replicaset");
    Ok(())
}

#[tokio::test]
async fn node_drills_to_a_strict_subset_of_pods() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };

    let total_pods = cluster
        .vista_factory()
        .from_table(pods::pods_table(cluster.clone()))?
        .get_count()
        .await?;

    let nodes_vista = cluster.vista_factory().from_table(nodes::nodes_table(cluster.clone()))?;
    let node_rows = nodes_vista.fetch_window(0, 10).await?;
    let (_, node) = node_rows.first().expect("at least one node");

    let pods_on_node = nodes_vista.get_ref("pods", node)?;
    let on_node = pods_on_node.fetch_window(0, 5000).await?;

    assert!(!on_node.is_empty(), "a Ready node should host some pods");
    // The relation narrows: a node hosts a subset of all pods, and every
    // returned pod actually runs on that node.
    assert!(
        (on_node.len() as i64) <= total_pods,
        "node pods ({}) should not exceed total pods ({total_pods})",
        on_node.len()
    );
    let node_name = as_text(node.get("name"));
    for (_, pod) in &on_node {
        assert_eq!(as_text(pod.get("nodeName")), node_name);
    }
    Ok(())
}
