//! Ready-made tables to skip the table-name dance.
//!
//! CloudWatch Logs (JSON-1.1, under [`logs`]):
//!   - [`logs::groups_table`]  — `DescribeLogGroups`
//!   - [`logs::streams_table`] — `DescribeLogStreams`
//!   - [`logs::events_table`]  — `FilterLogEvents`
//!
//! ECS (JSON-1.1, under [`ecs`]):
//!   - [`ecs::clusters_table`]
//!   - [`ecs::services_table`]
//!   - [`ecs::tasks_table`]
//!   - [`ecs::task_definitions_table`]
//!
//! IAM (Query, under [`iam`]):
//!   - [`iam::users_table`]              — `ListUsers`
//!   - [`iam::groups_table`]             — `ListGroups`
//!   - [`iam::roles_table`]              — `ListRoles`
//!   - [`iam::policies_table`]           — `ListPolicies`
//!   - [`iam::access_keys_table`]        — `ListAccessKeys`  (per user)
//!   - [`iam::instance_profiles_table`]  — `ListInstanceProfiles`
//!
//! ```no_run
//! # use vantage_aws::{AwsAccount, eq};
//! # use vantage_aws::models::logs::groups_table;
//! # async fn run() -> vantage_core::Result<()> {
//! let aws = AwsAccount::from_default()?;
//! let mut groups = groups_table(aws);
//! groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
//! # Ok(()) }
//! ```

pub mod ecs;
pub mod iam;
pub mod logs;
