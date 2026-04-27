use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One log event from `FilterLogEvents`. Timestamps are CloudWatch's
/// usual milliseconds-since-epoch.
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

/// `Logs_20140328.FilterLogEvents` — needs `logGroupName` to be set on
/// the table via a condition before `list` will succeed (AWS rejects
/// the call otherwise).
pub fn log_events_table(aws: AwsAccount) -> Table<AwsAccount, LogEvent> {
    Table::new("events:logs/Logs_20140328.FilterLogEvents", aws)
        .with_id_column("eventId")
        .with_column_of::<String>("logStreamName")
        .with_column_of::<i64>("timestamp")
        .with_column_of::<String>("message")
}
