//! [`KubernetesCluster`] — a handle to one Kubernetes cluster.
//!
//! Wraps a [`kube::Client`] (which owns config, TLS, and auth resolved
//! from the kubeconfig / in-cluster service account) plus a default
//! namespace. Cheap to clone — the client is `Arc`-backed. Used directly
//! as the `TableSource` for every K8s resource table; the resource's API
//! path lives in the table name (see [`crate::dispatch`]).

use std::sync::Arc;

use vantage_core::{Result, error};

#[derive(Clone)]
pub struct KubernetesCluster {
    inner: Arc<Inner>,
}

struct Inner {
    client: kube::Client,
    default_namespace: String,
}

impl KubernetesCluster {
    /// Build from an explicit [`kube::Client`].
    pub fn new(client: kube::Client) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                default_namespace: "default".to_string(),
            }),
        }
    }

    /// Build from the ambient configuration: the current context in
    /// `~/.kube/config` (honouring `$KUBECONFIG`), or the in-cluster
    /// service account when running inside a pod. This is what the CLI and
    /// integration tests use.
    pub async fn try_default() -> Result<Self> {
        install_crypto_provider();
        let client = kube::Client::try_default()
            .await
            .map_err(|e| error!("failed to build Kubernetes client from kubeconfig", details = e.to_string()))?;
        Ok(Self::new(client))
    }

    /// Alias for [`try_default`](Self::try_default), matching the
    /// `vantage-aws` `from_default` convention.
    pub async fn from_default() -> Result<Self> {
        Self::try_default().await
    }

    /// Return a copy whose default namespace is `namespace`. Only affects
    /// resources fetched through namespaced convenience paths; the raw
    /// per-resource list paths used by the models are cluster-wide.
    pub fn with_namespace(self, namespace: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                client: self.inner.client.clone(),
                default_namespace: namespace.into(),
            }),
        }
    }

    pub(crate) fn client(&self) -> &kube::Client {
        &self.inner.client
    }

    pub fn default_namespace(&self) -> &str {
        &self.inner.default_namespace
    }
}

/// rustls 0.23 needs a process-wide `CryptoProvider`. Install the ring
/// provider once; ignore the error if the host application already set one.
fn install_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl std::fmt::Debug for KubernetesCluster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KubernetesCluster")
            .field("default_namespace", &self.inner.default_namespace)
            .finish()
    }
}
