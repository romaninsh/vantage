//! ReplicaSets — `apis/apps/v1/replicasets`. Child of a Deployment (via
//! `ownerName` = owning Deployment), parent of its Pods (Pods carry the
//! ReplicaSet name as their `ownerName`).

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::models::core;
use crate::project::{self, Row};

pub const PATH: &str = "apis/apps/v1/replicasets";

pub fn replicasets_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("namespace")
        .with_column_of::<i64>("replicas")
        .with_column_of::<i64>("ready")
        .with_column_of::<i64>("available")
        .with_column_of::<String>("age")
        .with_many("pods", "ownerName", core::pods::pods_table)
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let name = project::str_at(item, "metadata.name").unwrap_or_default();
    let (owner_name, _) = project::owner(item);
    let record = Row::new()
        .text("id", name.clone())
        .text("name", name.clone())
        .str("namespace", item, "metadata.namespace")
        .int("replicas", item, "spec.replicas")
        .int("ready", item, "status.readyReplicas")
        .int("available", item, "status.availableReplicas")
        .opt_text("age", project::age(item))
        // Owning Deployment, for the deployment → replicasets join.
        .opt_text("ownerName", owner_name)
        .build();
    (name, record)
}
