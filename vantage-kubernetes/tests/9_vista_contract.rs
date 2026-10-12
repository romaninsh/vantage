//! `vantage_vista::contract` over live pods. Requires a seeded cluster and
//! `RUN_K8S_INTEGRATION=1`. Read-only: the backend advertises no writes.

mod common;

use ciborium::Value as CborValue;
use common::{NS, cluster_or_skip};
use vantage_kubernetes::models::core::pods;
use vantage_vista::contract;

#[tokio::test]
async fn kubernetes_contract_advertised() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let pods = cluster
        .vista_factory()
        .from_table(pods::pods_table(cluster.clone()))?;
    contract::check_advertised(&pods).await;
    Ok(())
}

#[tokio::test]
async fn kubernetes_contract_narrowing() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let pods = cluster
        .vista_factory()
        .from_table(pods::pods_table(cluster.clone()))?;
    contract::check_narrowing(&pods).await;
    Ok(())
}

/// The fixture namespace holds some of the cluster's pods; the system
/// namespaces hold the rest.
#[tokio::test]
async fn kubernetes_contract_clone() -> anyhow::Result<()> {
    let Some(cluster) = cluster_or_skip().await else {
        return Ok(());
    };
    let pods = cluster
        .vista_factory()
        .from_table(pods::pods_table(cluster.clone()))?;
    contract::check_clone(&pods, "namespace", CborValue::Text(NS.into())).await;
    Ok(())
}
