use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

use super::log_event::{LogEvent, log_events_table};

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
/// Comes with an `events` relation that traverses to the group's
/// log events. AWS doesn't accept multi-value filters, so the source
/// has to narrow to a single group before traversal — otherwise the
/// traversal errors at execute time.
///
/// ```no_run
/// # use vantage_aws::{AwsAccount, eq};
/// # use vantage_aws::models::log_groups_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let mut groups = log_groups_table(aws);
/// groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
/// # Ok(()) }
/// ```
pub fn log_groups_table(aws: AwsAccount) -> Table<AwsAccount, LogGroup> {
    Table::new("logGroups:logs/Logs_20140328.DescribeLogGroups", aws)
        .with_id_column("logGroupName")
        .with_column_of::<i64>("creationTime")
        .with_column_of::<i64>("storedBytes")
        .with_many("events", "logGroupName", log_events_table)
}

impl LogGroup {
    /// Events table pre-filtered to *this* group's name. Use when
    /// you already have a resolved `LogGroup` in hand and don't need
    /// to go through the table-level relation traversal.
    pub fn ref_events(&self, aws: AwsAccount) -> Table<AwsAccount, LogEvent> {
        let mut t = log_events_table(aws);
        t.add_condition(eq("logGroupName", self.log_group_name.clone()));
        t
    }
}
