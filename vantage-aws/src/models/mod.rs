//! Ready-made tables to skip the table-name dance.
//!
//! CloudWatch Logs:
//!   - [`log_groups_table`]  — `DescribeLogGroups`
//!   - [`log_streams_table`] — `DescribeLogStreams`
//!   - [`log_events_table`]  — `FilterLogEvents`
//!
//! ECS (under [`ecs`]):
//!   - [`ecs::clusters_table`]
//!   - [`ecs::services_table`]
//!   - [`ecs::tasks_table`]
//!   - [`ecs::task_definitions_table`]
//!
//! ```no_run
//! # use vantage_aws::{AwsAccount, eq};
//! # use vantage_aws::models::log_groups_table;
//! # async fn run() -> vantage_core::Result<()> {
//! let aws = AwsAccount::from_default()?;
//! let mut groups = log_groups_table(aws);
//! groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
//! # Ok(()) }
//! ```

pub mod ecs;
pub mod log_event;
pub mod log_group;
pub mod log_stream;

pub use log_event::{LogEvent, log_events_table};
pub use log_group::{LogGroup, log_groups_table};
pub use log_stream::{LogStream, log_streams_table};
