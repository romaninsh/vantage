//! Events — `api/v1/events`.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::cluster::KubernetesCluster;
use crate::project::{self, Row};

pub const PATH: &str = "api/v1/events";

pub fn events_table(cluster: KubernetesCluster) -> Table<KubernetesCluster, EmptyEntity> {
    Table::new(PATH, cluster)
        .with_id_column("id")
        .with_title_column_of::<String>("reason")
        .with_column_of::<String>("namespace")
        .with_column_of::<String>("type")
        .with_column_of::<String>("object")
        .with_column_of::<String>("message")
        .with_column_of::<i64>("count")
        .with_column_of::<String>("age")
}

fn involved_object(item: &JsonValue) -> Option<String> {
    let obj = item.get("involvedObject")?;
    let kind = obj.get("kind").and_then(|v| v.as_str())?;
    let name = obj.get("name").and_then(|v| v.as_str())?;
    Some(format!("{kind}/{name}"))
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
        .str("reason", item, "reason")
        .str("namespace", item, "metadata.namespace")
        .str("type", item, "type")
        .opt_text("object", involved_object(item))
        .str("message", item, "message")
        .int("count", item, "count")
        .opt_text("age", project::age(item))
        .build();
    (id, record)
}
