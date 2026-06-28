//! List dispatch for [`KubernetesCluster`].
//!
//! The table name is the resource's API path, e.g. `api/v1/pods` or
//! `apis/apps/v1/deployments`. We GET `{server}/{path}` through the
//! `kube::Client` (which signs/authenticates the request) and pull the
//! `items` array out of the `…List` envelope. Selector pushdown
//! (`?labelSelector=`, namespaced sub-paths) is a later optimisation —
//! v1 lists the collection and filters client-side.

use serde_json::Value as JsonValue;
use vantage_core::{Result, error};

use crate::cluster::KubernetesCluster;

impl KubernetesCluster {
    /// GET the list endpoint named by `api_path` and return its `items`.
    pub(crate) async fn list_items(&self, api_path: &str) -> Result<Vec<JsonValue>> {
        let path = normalize_path(api_path);
        let request = http::Request::builder()
            .method(http::Method::GET)
            .uri(&path)
            .body(Vec::new())
            .map_err(|e| error!("failed to build Kubernetes request", path = path.clone(), details = e.to_string()))?;

        let resp: JsonValue = self
            .client()
            .request(request)
            .await
            .map_err(|e| error!("Kubernetes list request failed", path = path.clone(), details = e.to_string()))?;

        match resp.get("items").and_then(|v| v.as_array()) {
            Some(items) => Ok(items.clone()),
            None => {
                // A non-list response (e.g. a `Status` error object) — surface
                // its message rather than silently returning nothing.
                let kind = resp.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let message = resp
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("response had no `items` array");
                Err(error!(
                    "Kubernetes list response was not a collection",
                    path = path,
                    kind = kind.to_string(),
                    message = message.to_string()
                ))
            }
        }
    }
}

/// Ensure exactly one leading slash. Table names are written without one
/// (`api/v1/pods`) to mirror the `vantage-aws` convention; the kube client
/// wants an absolute path.
fn normalize_path(api_path: &str) -> String {
    format!("/{}", api_path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::normalize_path;

    #[test]
    fn adds_leading_slash() {
        assert_eq!(normalize_path("api/v1/pods"), "/api/v1/pods");
    }

    #[test]
    fn collapses_existing_leading_slash() {
        assert_eq!(normalize_path("/apis/apps/v1/deployments"), "/apis/apps/v1/deployments");
    }
}
