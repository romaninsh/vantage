//! Services — `api/v1/services`.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};

pub const PATH: &str = "api/v1/services";

pub fn services_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("namespace")
        .with_column_of::<String>("type")
        .with_column_of::<String>("clusterIP")
        .with_column_of::<String>("ports")
        .with_column_of::<String>("age")
}

/// `spec.ports` as a compact `port/protocol` summary.
fn ports(item: &JsonValue) -> Option<String> {
    let ports = item.get("spec")?.get("ports")?.as_array()?;
    let parts: Vec<String> = ports
        .iter()
        .filter_map(|p| {
            let port = p.get("port").and_then(|v| v.as_i64())?;
            let proto = p.get("protocol").and_then(|v| v.as_str()).unwrap_or("TCP");
            Some(format!("{port}/{proto}"))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(","))
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let id = project::str_at(item, "metadata.uid")
        .or_else(|| match (project::str_at(item, "metadata.namespace"), project::str_at(item, "metadata.name")) {
            (Some(ns), Some(name)) => Some(format!("{ns}/{name}")),
            _ => None,
        })
        .unwrap_or_default();
    let record = Row::new()
        .text("id", id.clone())
        .str("name", item, "metadata.name")
        .str("namespace", item, "metadata.namespace")
        .str("type", item, "spec.type")
        .str("clusterIP", item, "spec.clusterIP")
        .opt_text("ports", ports(item))
        .opt_text("age", project::age(item))
        .build();
    (id, record)
}
