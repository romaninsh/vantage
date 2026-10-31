//! `vantage_vista::contract` over live pods. Requires a seeded cluster and
//! `RUN_K8S_INTEGRATION=1`. Read-only: the backend advertises no writes.

mod common;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use common::{NS, cluster_or_skip};
use vantage_kubernetes::KubernetesCluster;
use vantage_kubernetes::models::core::{namespaces, pods};
use vantage_vista::contract::{self, TxFixture, TxTables};

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

/// The API server has no transactions, and the backend takes no writes.
struct Cluster(KubernetesCluster);

#[async_trait]
impl TxFixture for Cluster {
    async fn fresh(&self) -> TxTables {
        let factory = self.0.vista_factory();
        TxTables::new(
            factory
                .from_table(pods::pods_table(self.0.clone()))
                .unwrap(),
            factory
                .from_table(namespaces::namespaces_table(self.0.clone()))
                .unwrap(),
        )
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }

    fn detects_deadlocks(&self) -> bool {
        false
    }

    fn supports_transactions(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn kubernetes_transactions_hold_the_contract() {
    let Some(cluster) = cluster_or_skip().await else {
        return;
    };
    contract::check_transactions(&Cluster(cluster)).await
}
