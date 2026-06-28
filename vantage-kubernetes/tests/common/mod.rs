//! Shared helpers for the live integration tests.
//!
//! These tests require a reachable cluster seeded by `./scripts/start.sh`
//! and `./scripts/ingress.sh`. They run only when `RUN_K8S_INTEGRATION=1`
//! is set, so a plain `cargo test` stays green on a machine without a
//! cluster. CI sets the variable after standing minikube up.
//!
// Each test binary compiles this module but uses only part of it.
#![allow(dead_code)]

use ciborium::Value as CborValue;
use vantage_kubernetes::KubernetesCluster;

/// The namespace the Helm fixtures install into.
pub const NS: &str = "demo";

/// Replicas the `web` Deployment runs (must match `scripts/chart/values.yaml`).
pub const WEB_REPLICAS: usize = 3;

/// Connect to the cluster, or return `None` (skipping the test) when
/// integration tests aren't enabled or no cluster is reachable.
pub async fn cluster_or_skip() -> Option<KubernetesCluster> {
    if std::env::var("RUN_K8S_INTEGRATION").ok().as_deref() != Some("1") {
        eprintln!("skipping: set RUN_K8S_INTEGRATION=1 to run live cluster tests");
        return None;
    }
    match KubernetesCluster::from_default().await {
        Ok(cluster) => Some(cluster),
        Err(e) => {
            eprintln!("skipping: no reachable cluster ({e})");
            None
        }
    }
}

/// Extract a `Text` CBOR value as an owned `String`.
pub fn as_text(value: Option<&CborValue>) -> Option<String> {
    match value {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}
