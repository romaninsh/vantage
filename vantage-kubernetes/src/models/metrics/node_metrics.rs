//! Node metrics — `apis/metrics.k8s.io/v1beta1/nodes` (metrics-server).
//! Instantaneous CPU/memory usage, parsed into numbers for charts.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};

pub const PATH: &str = "apis/metrics.k8s.io/v1beta1/nodes";

pub fn node_metrics_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<i64>("cpuMillicores")
        .with_column_of::<i64>("memBytes")
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let name = project::str_at(item, "metadata.name").unwrap_or_default();
    let record = Row::new()
        .text("id", name.clone())
        .text("name", name.clone())
        .cpu_millicores("cpuMillicores", item, "usage.cpu")
        .memory_bytes("memBytes", item, "usage.memory")
        .build();
    (name, record)
}
