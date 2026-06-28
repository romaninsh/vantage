//! Nodes — `api/v1/nodes`. Cluster machines; parent of their pods.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};

pub const PATH: &str = "api/v1/nodes";

pub fn nodes_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<bool>("ready")
        .with_column_of::<String>("roles")
        .with_column_of::<String>("version")
        .with_column_of::<String>("internalIP")
        .with_column_of::<i64>("cpuCapacityMillicores")
        .with_column_of::<i64>("memCapacityBytes")
        .with_column_of::<String>("age")
        .with_many("pods", "nodeName", super::pods::pods_table)
}

/// Ready = the `Ready` condition has `status == "True"`.
fn node_ready(item: &JsonValue) -> bool {
    item.get("status")
        .and_then(|s| s.get("conditions"))
        .and_then(|c| c.as_array())
        .map(|conds| {
            conds.iter().any(|c| {
                c.get("type").and_then(|v| v.as_str()) == Some("Ready")
                    && c.get("status").and_then(|v| v.as_str()) == Some("True")
            })
        })
        .unwrap_or(false)
}

/// The first `InternalIP` from `status.addresses`.
fn internal_ip(item: &JsonValue) -> Option<String> {
    item.get("status")
        .and_then(|s| s.get("addresses"))
        .and_then(|a| a.as_array())
        .and_then(|addrs| {
            addrs.iter().find_map(|a| {
                (a.get("type").and_then(|v| v.as_str()) == Some("InternalIP"))
                    .then(|| a.get("address").and_then(|v| v.as_str()).map(str::to_string))
                    .flatten()
            })
        })
}

/// Roles from `node-role.kubernetes.io/<role>` labels, comma-joined.
fn roles(item: &JsonValue) -> Option<String> {
    let labels = item.get("metadata")?.get("labels")?.as_object()?;
    let mut roles: Vec<&str> = labels
        .keys()
        .filter_map(|k| k.strip_prefix("node-role.kubernetes.io/"))
        .filter(|r| !r.is_empty())
        .collect();
    roles.sort_unstable();
    if roles.is_empty() {
        Some("<none>".to_string())
    } else {
        Some(roles.join(","))
    }
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let name = project::str_at(item, "metadata.name").unwrap_or_default();
    let record = Row::new()
        .text("id", name.clone())
        .text("name", name.clone())
        .set("ready", CborValue::Bool(node_ready(item)))
        .opt_text("roles", roles(item))
        .str("version", item, "status.nodeInfo.kubeletVersion")
        .opt_text("internalIP", internal_ip(item))
        .cpu_millicores("cpuCapacityMillicores", item, "status.capacity.cpu")
        .memory_bytes("memCapacityBytes", item, "status.capacity.memory")
        .opt_text("age", project::age(item))
        .build();
    (name, record)
}
