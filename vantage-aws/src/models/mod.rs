//! Two ready-made CloudWatch tables to skip the table-name dance.
//!
//! - [`log_groups_table`] — `DescribeLogGroups`
//! - [`log_events_table`] — `FilterLogEvents`
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

pub mod log_event;
pub mod log_group;

pub use log_event::{LogEvent, log_events_table};
pub use log_group::{LogGroup, log_groups_table};
