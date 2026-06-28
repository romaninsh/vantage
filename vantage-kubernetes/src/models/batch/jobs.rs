//! Jobs — `apis/batch/v1/jobs`.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};

pub const PATH: &str = "apis/batch/v1/jobs";

pub fn jobs_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("namespace")
        .with_column_of::<i64>("completions")
        .with_column_of::<i64>("succeeded")
        .with_column_of::<i64>("active")
        .with_column_of::<i64>("failed")
        .with_column_of::<String>("age")
}

pub fn project(item: &JsonValue) -> (String, Record<CborValue>) {
    let name = project::str_at(item, "metadata.name").unwrap_or_default();
    let record = Row::new()
        .text("id", name.clone())
        .text("name", name.clone())
        .str("namespace", item, "metadata.namespace")
        .int("completions", item, "spec.completions")
        .int("succeeded", item, "status.succeeded")
        .int("active", item, "status.active")
        .int("failed", item, "status.failed")
        .opt_text("age", project::age(item))
        .build();
    (name, record)
}
