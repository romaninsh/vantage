//! Ready-made ECS tables — clusters, services, tasks, task definitions.
//!
//! AWS's ECS APIs split into a `List*` step (returns ARNs only) and a
//! `Describe*` step (takes those ARNs, returns full objects). v0
//! exposes the `List*` side: rows hold the ARN plus a parsed-out
//! short name where it's useful.
//!
//! ```no_run
//! # use vantage_aws::AwsAccount;
//! # use vantage_aws::models::ecs::clusters_table;
//! # use vantage_dataset::prelude::ReadableValueSet;
//! # async fn run() -> vantage_core::Result<()> {
//! # let aws = AwsAccount::from_default()?;
//! let clusters = clusters_table(aws.clone());
//! for cluster in clusters.list_values().await? {
//!     // …
//! }
//! # Ok(()) }
//! ```

pub mod cluster;
pub mod service;
pub mod task;
pub mod task_definition;

pub use cluster::{Cluster, clusters_table};
pub use service::{Service, services_table};
pub use task::{Task, tasks_table};
pub use task_definition::{TaskDefinition, task_definitions_table};
