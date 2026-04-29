use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One log event. Timestamps are CloudWatch's usual
/// milliseconds-since-epoch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    #[serde(rename = "eventId")]
    pub event_id: String,
    #[serde(rename = "logStreamName", default)]
    pub log_stream_name: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub message: String,
}

/// `FilterLogEvents` table. AWS requires `logGroupName` before it
/// will list anything, so add `eq("logGroupName", "...")` before
/// calling `list`.
///
/// ```no_run
/// # use vantage_aws::{AwsAccount, eq};
/// # use vantage_aws::models::logs::events_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let mut events = events_table(aws);
/// events.add_condition(eq("logGroupName", "/aws/lambda/foo"));
/// # Ok(()) }
/// ```
pub fn events_table(aws: AwsAccount) -> Table<AwsAccount, LogEvent> {
    Table::new("json1/events:logs/Logs_20140328.FilterLogEvents", aws)
        .with_id_column("eventId")
        .with_column_of::<String>("logStreamName")
        .with_column_of::<i64>("timestamp")
        .with_column_of::<String>("message")
}
