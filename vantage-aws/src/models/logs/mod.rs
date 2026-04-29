//! Ready-made CloudWatch Logs tables — log groups, log streams, log
//! events. All speak JSON-1.1.
//!
//! ```no_run
//! # use vantage_aws::{AwsAccount, eq};
//! # use vantage_aws::models::logs::groups_table;
//! # async fn run() -> vantage_core::Result<()> {
//! # let aws = AwsAccount::from_default()?;
//! let mut groups = groups_table(aws);
//! groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
//! # Ok(()) }
//! ```

pub mod event;
pub mod group;
pub mod stream;

pub use event::{LogEvent, events_table};
pub use group::{LogGroup, groups_table};
pub use stream::{LogStream, streams_table};
