//! Namespaces — `api/v1/namespaces`. Cluster-scoped; parent of everything
//! namespaced. Its id is the namespace name, which children carry as their
//! `namespace` field, so every `with_many` here narrows correctly.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::models::{apps, batch, core};
use crate::project::{self, Row};

pub const PATH: &str = "api/v1/namespaces";

pub fn namespaces_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("phase")
        .with_column_of::<String>("age")
        .with_many("pods", "namespace", core::pods::pods_table)
        .with_many("deployments", "namespace", apps::deployments::deployments_table)
        .with_many("replicasets", "namespace", apps::replicasets::replicasets_table)
        .with_many("services", "namespace", core::services::services_table)
        .with_many("jobs", "namespace", batch::jobs::jobs_table)
        .with_many("configmaps", "namespace", core::configmaps::configmaps_table)
        .with_many("secrets", "namespace", core::secrets::secrets_table)
        .with_many("events", "namespace", core::events::events_table)
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let name = project::str_at(item, "metadata.name").unwrap_or_default();
    let record = Row::new()
        .text("id", name.clone())
        .text("name", name.clone())
        .str("phase", item, "status.phase")
        .opt_text("age", project::age(item))
        .build();
    (name, record)
}
