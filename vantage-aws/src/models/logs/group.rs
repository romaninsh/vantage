use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

use super::event::{LogEvent, events_table};
use super::stream::{LogStream, streams_table};

/// One CloudWatch Logs group. Field names match the wire shape —
/// these are exactly what `DescribeLogGroups` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogGroup {
    #[serde(rename = "logGroupName")]
    pub log_group_name: String,
    #[serde(rename = "creationTime", default)]
    pub creation_time: i64,
    #[serde(rename = "storedBytes", default)]
    pub stored_bytes: i64,
}

/// `DescribeLogGroups` table — every log group in the account. Filter
/// by adding `eq("logGroupNamePrefix", "...")`.
///
/// Two relations:
///   - `events` → `FilterLogEvents` for this group
///   - `streams` → `DescribeLogStreams` for this group
///
/// AWS doesn't accept multi-value filters, so the source has to narrow
/// to a single group before traversal — otherwise the call errors at
/// execute time.
///
/// ```no_run
/// # use vantage_aws::{AwsAccount, eq};
/// # use vantage_aws::models::logs::groups_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let mut groups = groups_table(aws);
/// groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
/// # Ok(()) }
/// ```
pub fn groups_table(aws: AwsAccount) -> Table<AwsAccount, LogGroup> {
    Table::new("json1/logGroups:logs/Logs_20140328.DescribeLogGroups", aws)
        .with_id_column("logGroupName")
        .with_column_of::<i64>("creationTime")
        .with_title_column_of::<i64>("storedBytes")
        .with_many("events", "logGroupName", events_table)
        .with_many("streams", "logGroupName", streams_table)
}

impl LogGroup {
    /// Build a [`groups_table`] narrowed to the group named in `arn`.
    ///
    /// Accepts ARNs of the shape
    /// `arn:aws:logs:<region>:<account>:log-group:<name>` with an
    /// optional trailing `:*` (the form returned by IAM policy
    /// resources). Returns `None` if the ARN isn't a log-group ARN.
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, LogGroup>> {
        let after = arn.split(":log-group:").nth(1)?;
        // Strip the optional :log-stream:... or :* suffix.
        let name = after
            .split(":log-stream:")
            .next()
            .unwrap_or(after)
            .trim_end_matches(":*");
        if name.is_empty() {
            return None;
        }
        let mut t = groups_table(aws);
        t.add_condition(eq("logGroupNamePrefix", name.to_string()));
        Some(t)
    }

    /// Events table pre-filtered to *this* group's name.
    pub fn ref_events(&self, aws: AwsAccount) -> Table<AwsAccount, LogEvent> {
        let mut t = events_table(aws);
        t.add_condition(eq("logGroupName", self.log_group_name.clone()));
        t
    }

    /// Streams table pre-filtered to *this* group's name.
    pub fn ref_streams(&self, aws: AwsAccount) -> Table<AwsAccount, LogStream> {
        let mut t = streams_table(aws);
        t.add_condition(eq("logGroupName", self.log_group_name.clone()));
        t
    }
}
