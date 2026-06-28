//! Pods — `api/v1/pods`. The leaf of most drill-downs.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};

pub const PATH: &str = "api/v1/pods";

pub fn pods_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("namespace")
        .with_column_of::<String>("nodeName")
        .with_column_of::<String>("phase")
        .with_column_of::<String>("ready")
        .with_column_of::<i64>("restarts")
        .with_column_of::<String>("podIP")
        .with_column_of::<String>("age")
}

/// Build `(readyContainers, restartTotal)` from `status.containerStatuses`.
fn container_summary(item: &JsonValue) -> (String, i64) {
    let statuses = item
        .get("status")
        .and_then(|s| s.get("containerStatuses"))
        .and_then(|c| c.as_array());
    let Some(statuses) = statuses else {
        return ("0/0".to_string(), 0);
    };
    let total = statuses.len();
    let ready = statuses
        .iter()
        .filter(|c| c.get("ready").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();
    let restarts: i64 = statuses
        .iter()
        .filter_map(|c| c.get("restartCount").and_then(|v| v.as_i64()))
        .sum();
    (format!("{ready}/{total}"), restarts)
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let id = project::str_at(item, "metadata.uid")
        .or_else(|| match (project::str_at(item, "metadata.namespace"), project::str_at(item, "metadata.name")) {
            (Some(ns), Some(name)) => Some(format!("{ns}/{name}")),
            _ => None,
        })
        .unwrap_or_default();
    let (ready, restarts) = container_summary(item);
    let (owner_name, owner_kind) = project::owner(item);

    let record = Row::new()
        .text("id", id.clone())
        .str("name", item, "metadata.name")
        .str("namespace", item, "metadata.namespace")
        .str("nodeName", item, "spec.nodeName")
        .str("phase", item, "status.phase")
        .text("ready", ready)
        .num("restarts", restarts)
        .str("podIP", item, "status.podIP")
        .opt_text("age", project::age(item))
        // Undeclared join keys — available for relation filtering, not shown.
        .opt_text("app", project::label(item, "app"))
        .opt_text("ownerName", owner_name)
        .opt_text("ownerKind", owner_kind)
        .opt_text("ownerDeployment", project::owner_deployment(item))
        .build();
    (id, record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciborium::Value::Text;

    fn fixture() -> JsonValue {
        serde_json::json!({
            "metadata": {
                "uid": "abc-123",
                "name": "web-5d9f8c-q4n2x",
                "namespace": "demo",
                "labels": { "app": "web", "pod-template-hash": "5d9f8c" },
                "ownerReferences": [ { "kind": "ReplicaSet", "name": "web-5d9f8c" } ],
                "creationTimestamp": "2026-06-27T10:00:00Z"
            },
            "spec": { "nodeName": "minikube" },
            "status": {
                "phase": "Running",
                "podIP": "10.244.0.7",
                "containerStatuses": [
                    { "ready": true, "restartCount": 0 },
                    { "ready": false, "restartCount": 3 }
                ]
            }
        })
    }

    #[test]
    fn projects_flat_pod_record() {
        let (id, rec) = project(&fixture());
        assert_eq!(id, "abc-123");
        assert_eq!(rec.get("name"), Some(&Text("web-5d9f8c-q4n2x".into())));
        assert_eq!(rec.get("namespace"), Some(&Text("demo".into())));
        assert_eq!(rec.get("nodeName"), Some(&Text("minikube".into())));
        assert_eq!(rec.get("phase"), Some(&Text("Running".into())));
        // 1 of 2 containers ready; restarts summed across containers.
        assert_eq!(rec.get("ready"), Some(&Text("1/2".into())));
        assert_eq!(rec.get("restarts"), Some(&CborValue::from(3i64)));
        // Join keys: app label, ReplicaSet owner, and the Deployment name
        // recovered by stripping the pod-template-hash.
        assert_eq!(rec.get("app"), Some(&Text("web".into())));
        assert_eq!(rec.get("ownerName"), Some(&Text("web-5d9f8c".into())));
        assert_eq!(rec.get("ownerDeployment"), Some(&Text("web".into())));
    }

    #[test]
    fn pod_with_no_containers_is_zero_ready() {
        let item = serde_json::json!({
            "metadata": { "uid": "x", "name": "pending", "namespace": "demo" },
            "status": { "phase": "Pending" }
        });
        let (_, rec) = project(&item);
        assert_eq!(rec.get("ready"), Some(&Text("0/0".into())));
        assert_eq!(rec.get("restarts"), Some(&CborValue::from(0i64)));
        // No owner → no deployment join key, so it won't leak into a
        // deployment's pod list.
        assert_eq!(rec.get("ownerDeployment"), None);
    }
}
