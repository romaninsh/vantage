//! Pod metrics — `apis/metrics.k8s.io/v1beta1/pods` (metrics-server).
//! Per-pod usage summed across its containers.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};
use crate::types::quantity;

pub const PATH: &str = "apis/metrics.k8s.io/v1beta1/pods";

pub fn pod_metrics_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("namespace")
        .with_column_of::<i64>("cpuMillicores")
        .with_column_of::<i64>("memBytes")
}

/// Sum a usage field across all containers, parsing each with `parse`.
fn sum_usage(item: &JsonValue, field: &str, parse: fn(&str) -> Option<i64>) -> i64 {
    item.get("containers")
        .and_then(|c| c.as_array())
        .map(|containers| {
            containers
                .iter()
                .filter_map(|c| c.get("usage")?.get(field)?.as_str().and_then(parse))
                .sum()
        })
        .unwrap_or(0)
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let id = match (project::str_at(item, "metadata.namespace"), project::str_at(item, "metadata.name")) {
        (Some(ns), Some(name)) => format!("{ns}/{name}"),
        _ => project::str_at(item, "metadata.name").unwrap_or_default(),
    };
    let record = Row::new()
        .text("id", id.clone())
        .str("name", item, "metadata.name")
        .str("namespace", item, "metadata.namespace")
        .num("cpuMillicores", sum_usage(item, "cpu", quantity::parse_cpu_millicores))
        .num("memBytes", sum_usage(item, "memory", quantity::parse_memory_bytes))
        .build();
    (id, record)
}
