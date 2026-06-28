//! Kubernetes API backend for Vantage — incubating.
//!
//! Treat the Kubernetes API as a Vantage `TableSource`. Build a
//! [`KubernetesCluster`] from your kubeconfig, hand it to a `Table`, and
//! the table name carries the resource's API path:
//!
//! ```text
//! api/v1/pods
//! apis/apps/v1/deployments
//! apis/metrics.k8s.io/v1beta1/nodes
//! ```
//!
//! List responses come back enveloped as `{ "items": [ … ] }`. Each item
//! is run through a per-resource *projector* that flattens the nested
//! object (`metadata.name`, `status.phase`, `spec.nodeName`), derives
//! array-backed fields (`ready` "2/3", restart counts, node IPs), and
//! parses K8s quantities (`16331752Ki`, `250m`) into numbers — so columns,
//! relations, and charts all see a flat, typed record.
//!
//! Conditions narrow the listing post-fetch (the cluster is small); v0 is
//! read-only. Ready-made tables live under [`models`].

mod cluster;
mod condition;
mod dispatch;
mod impls;
mod project;

pub mod models;
pub mod types;
pub mod vista;

pub use cluster::KubernetesCluster;
pub use condition::{KubeCondition, eq, in_};
