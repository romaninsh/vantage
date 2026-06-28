//! Vista bridge for the Kubernetes backend.
//!
//! Construct a `Vista` from a typed `Table<KubernetesCluster, E>` via
//! `cluster.vista_factory().from_table(...)`. Read-only in v1 — the shell
//! advertises only `can_count`.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::KubeVistaFactory;
pub use source::KubeTableShell;
pub use spec::{KubeColumnExtras, KubeTableExtras, KubeVistaSpec};

use crate::cluster::KubernetesCluster;

impl KubernetesCluster {
    /// Return a Vista factory bound to this cluster.
    pub fn vista_factory(&self) -> KubeVistaFactory {
        KubeVistaFactory::new(self.clone())
    }
}
