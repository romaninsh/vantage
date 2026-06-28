//! Ready-made tables for standard Kubernetes resources, plus the projector
//! dispatch and a generic name→[`Vista`] [`Factory`] used by the CLI.
//!
//! Each resource module exposes `PATH` (its API list path), a `*_table`
//! constructor (columns + relations), and a `project` function (raw object
//! → flat record). [`project_for`] routes a fetched object to the right
//! projector by API path; [`Factory`] routes a dotted name (`core.pods`,
//! `apps.deployments`) to a fully-built `Vista`.

pub mod apps;
pub mod batch;
pub mod core;
pub mod metrics;

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_types::Record;
use vantage_vista::Vista;

use crate::cluster::KubernetesCluster;

/// Project a fetched object using the projector registered for `api_path`.
/// Returns `None` for an unknown path (the table source then yields no rows
/// rather than guessing a shape).
pub(crate) fn project_for(api_path: &str, item: &JsonValue) -> Option<(String, Record<CborValue>)> {
    let project: fn(&JsonValue) -> (String, Record<CborValue>) = match api_path {
        core::pods::PATH => core::pods::project,
        core::nodes::PATH => core::nodes::project,
        core::namespaces::PATH => core::namespaces::project,
        core::services::PATH => core::services::project,
        core::configmaps::PATH => core::configmaps::project,
        core::secrets::PATH => core::secrets::project,
        core::events::PATH => core::events::project,
        apps::deployments::PATH => apps::deployments::project,
        apps::replicasets::PATH => apps::replicasets::project,
        batch::jobs::PATH => batch::jobs::project,
        metrics::node_metrics::PATH => metrics::node_metrics::project,
        metrics::pod_metrics::PATH => metrics::pod_metrics::project,
        _ => return None,
    };
    Some(project(item))
}

/// Whether a [`Factory`] lookup lists every match or returns just the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactoryMode {
    List,
    Single,
}

/// Generic, type-erased model factory: dotted name → [`Vista`].
#[derive(Debug, Clone)]
pub struct Factory {
    cluster: KubernetesCluster,
}

impl Factory {
    pub fn new(cluster: KubernetesCluster) -> Self {
        Self { cluster }
    }

    /// All known model names, plural and singular, in menu order.
    pub fn known_names() -> &'static [&'static str] {
        &[
            "core.nodes",
            "core.namespaces",
            "core.pods",
            "core.services",
            "core.configmaps",
            "core.secrets",
            "core.events",
            "apps.deployments",
            "apps.replicasets",
            "batch.jobs",
            "metrics.node_metrics",
            "metrics.pod_metrics",
        ]
    }

    /// Resolve a model name to a `Vista` plus its natural mode. Accepts the
    /// plural (list) and singular (single-record) forms.
    pub fn for_name(&self, name: &str) -> Option<(Vista, FactoryMode)> {
        let cluster = self.cluster.clone();
        let f = cluster.vista_factory();

        macro_rules! entry {
            ($table:path) => {
                f.from_table($table(cluster.clone())).ok()?
            };
        }

        let (vista, mode) = match name {
            "core.pods" => (entry!(core::pods::pods_table), FactoryMode::List),
            "core.pod" => (entry!(core::pods::pods_table), FactoryMode::Single),
            "core.nodes" => (entry!(core::nodes::nodes_table), FactoryMode::List),
            "core.node" => (entry!(core::nodes::nodes_table), FactoryMode::Single),
            "core.namespaces" => (entry!(core::namespaces::namespaces_table), FactoryMode::List),
            "core.namespace" => (entry!(core::namespaces::namespaces_table), FactoryMode::Single),
            "core.services" => (entry!(core::services::services_table), FactoryMode::List),
            "core.service" => (entry!(core::services::services_table), FactoryMode::Single),
            "core.configmaps" => (entry!(core::configmaps::configmaps_table), FactoryMode::List),
            "core.configmap" => (entry!(core::configmaps::configmaps_table), FactoryMode::Single),
            "core.secrets" => (entry!(core::secrets::secrets_table), FactoryMode::List),
            "core.secret" => (entry!(core::secrets::secrets_table), FactoryMode::Single),
            "core.events" => (entry!(core::events::events_table), FactoryMode::List),
            "core.event" => (entry!(core::events::events_table), FactoryMode::Single),
            "apps.deployments" => (entry!(apps::deployments::deployments_table), FactoryMode::List),
            "apps.deployment" => (entry!(apps::deployments::deployments_table), FactoryMode::Single),
            "apps.replicasets" => (entry!(apps::replicasets::replicasets_table), FactoryMode::List),
            "apps.replicaset" => (entry!(apps::replicasets::replicasets_table), FactoryMode::Single),
            "batch.jobs" => (entry!(batch::jobs::jobs_table), FactoryMode::List),
            "batch.job" => (entry!(batch::jobs::jobs_table), FactoryMode::Single),
            "metrics.node_metrics" => (entry!(metrics::node_metrics::node_metrics_table), FactoryMode::List),
            "metrics.pod_metrics" => (entry!(metrics::pod_metrics::pod_metrics_table), FactoryMode::List),
            _ => return None,
        };
        Some((vista, mode))
    }
}
