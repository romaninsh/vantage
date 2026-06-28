//! Deployments — `apis/apps/v1/deployments`. Parent of its ReplicaSets
//! and (transitively) Pods. Its id is the Deployment name: ReplicaSets
//! carry it as their owner name, and Pods carry it as `ownerDeployment`
//! (recovered from the ReplicaSet name), so both relations narrow cleanly
//! without depending on label conventions.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::models::core;
use crate::project::{self, Row};

pub const PATH: &str = "apis/apps/v1/deployments";

pub fn deployments_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("namespace")
        .with_column_of::<i64>("replicas")
        .with_column_of::<i64>("ready")
        .with_column_of::<i64>("updated")
        .with_column_of::<i64>("available")
        .with_column_of::<String>("age")
        .with_many("replicasets", "ownerName", super::replicasets::replicasets_table)
        .with_many("pods", "ownerDeployment", core::pods::pods_table)
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let name = project::str_at(item, "metadata.name").unwrap_or_default();
    let record = Row::new()
        .text("id", name.clone())
        .text("name", name.clone())
        .str("namespace", item, "metadata.namespace")
        .int("replicas", item, "spec.replicas")
        .int("ready", item, "status.readyReplicas")
        .int("updated", item, "status.updatedReplicas")
        .int("available", item, "status.availableReplicas")
        .opt_text("age", project::age(item))
        .build();
    (name, record)
}
