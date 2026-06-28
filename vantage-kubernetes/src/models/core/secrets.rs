//! Secrets — `api/v1/secrets`. Metadata only; values are never projected.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};

pub const PATH: &str = "api/v1/secrets";

pub fn secrets_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("name")
        .with_column_of::<String>("namespace")
        .with_column_of::<String>("type")
        .with_column_of::<i64>("keys")
        .with_column_of::<String>("age")
}

fn key_count(item: &JsonValue) -> i64 {
    item.get("data")
        .and_then(|d| d.as_object())
        .map(|o| o.len() as i64)
        .unwrap_or(0)
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
        .str("type", item, "type")
        .num("keys", key_count(item))
        .opt_text("age", project::age(item))
        .build();
    (id, record)
}
