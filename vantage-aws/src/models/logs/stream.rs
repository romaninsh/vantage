use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{eq, AwsAccount};

use super::event::{events_table, LogEvent};

/// One CloudWatch Logs stream from `DescribeLogStreams`. Field names
/// match the wire shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogStream {
    #[serde(rename = "logStreamName")]
    pub log_stream_name: String,
    #[serde(default)]
    pub arn: String,
    #[serde(rename = "creationTime", default)]
    pub creation_time: i64,
    #[serde(rename = "firstEventTimestamp", default)]
    pub first_event_timestamp: i64,
    #[serde(rename = "lastEventTimestamp", default)]
    pub last_event_timestamp: i64,
    #[serde(rename = "lastIngestionTime", default)]
    pub last_ingestion_time: i64,
    #[serde(rename = "storedBytes", default)]
    pub stored_bytes: i64,
    #[serde(rename = "uploadSequenceToken", default)]
    pub upload_sequence_token: String,
}

/// `DescribeLogStreams` table. AWS requires `logGroupName` before it
/// will list anything, so add `eq("logGroupName", "...")` first.
///
/// ```no_run
/// # use vantage_aws::{AwsAccount, eq};
/// # use vantage_aws::models::logs::streams_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let mut streams = streams_table(aws);
/// streams.add_condition(eq("logGroupName", "/aws/lambda/foo"));
/// # Ok(()) }
/// ```
pub fn streams_table(aws: AwsAccount) -> Table<AwsAccount, LogStream> {
    Table::new(
        "json1/logStreams:logs/Logs_20140328.DescribeLogStreams",
        aws,
    )
    .with_id_column("logStreamName")
    .with_column_of::<String>("arn")
    .with_column_of::<i64>("creationTime")
    .with_column_of::<i64>("firstEventTimestamp")
    .with_title_column_of::<i64>("lastEventTimestamp")
    .with_column_of::<i64>("lastIngestionTime")
    .with_title_column_of::<i64>("storedBytes")
    .with_column_of::<String>("uploadSequenceToken")
}

impl LogStream {
    /// Build a [`streams_table`] narrowed to the stream identified by
    /// `arn`. Accepts ARNs of the shape
    /// `arn:aws:logs:<region>:<account>:log-group:<group>:log-stream:<stream>`.
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, LogStream>> {
        let after_group = arn.split(":log-group:").nth(1)?;
        let (group_name, stream_name) = after_group.split_once(":log-stream:")?;
        if group_name.is_empty() || stream_name.is_empty() {
            return None;
        }
        let mut t = streams_table(aws);
        t.add_condition(eq("logGroupName", group_name.to_string()));
        t.add_condition(eq("logStreamNamePrefix", stream_name.to_string()));
        Some(t)
    }

    /// The owning log group's name, parsed out of [`Self::arn`].
    /// Stream ARNs have the shape
    /// `arn:aws:logs:<region>:<account>:log-group:<group>:log-stream:<stream>`.
    pub fn log_group_name(&self) -> Option<&str> {
        let after = self.arn.split(":log-group:").nth(1)?;
        after.split(":log-stream:").next()
    }

    /// Events table pre-filtered to *this* stream. The log group is
    /// pulled from this stream's ARN; if the ARN is empty, pass it
    /// in via [`Self::ref_events_in`] instead.
    pub fn ref_events(&self, aws: AwsAccount) -> Option<Table<AwsAccount, LogEvent>> {
        let group = self.log_group_name()?;
        Some(self.ref_events_in(aws, group))
    }

    /// Events table pre-filtered to *this* stream within the given
    /// log group. Use when the stream ARN isn't populated (e.g.
    /// streams synthesised by hand).
    pub fn ref_events_in(
        &self,
        aws: AwsAccount,
        log_group_name: &str,
    ) -> Table<AwsAccount, LogEvent> {
        let mut t = events_table(aws);
        t.add_condition(eq("logGroupName", log_group_name));
        t.add_condition(eq("logStreamNamePrefix", self.log_stream_name.clone()));
        t
    }
}
