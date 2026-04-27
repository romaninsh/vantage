use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

use super::log_event::{LogEvent, log_events_table};

/// One CloudWatch Logs group. Field naming follows the JSON-1.1 wire
/// shape — these are exactly what `DescribeLogGroups` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogGroup {
    #[serde(rename = "logGroupName")]
    pub log_group_name: String,
    #[serde(rename = "creationTime", default)]
    pub creation_time: i64,
    #[serde(rename = "storedBytes", default)]
    pub stored_bytes: i64,
}

/// `Logs_20140328.DescribeLogGroups` — list all log groups in the
/// account. Filter by adding `eq("logGroupNamePrefix", "...")`.
///
/// `events` is registered as a `with_many` relation: traversing it
/// builds a `FilterLogEvents` table whose `logGroupName` matches the
/// source's id column. The framework's `related_in_condition` machinery
/// produces a deferred subquery; AWS rejects multi-value filters, so
/// the source must narrow to exactly one row before traversal —
/// otherwise the query errors at execute time.
pub fn log_groups_table(aws: AwsAccount) -> Table<AwsAccount, LogGroup> {
    Table::new("logGroups:logs/Logs_20140328.DescribeLogGroups", aws)
        .with_id_column("logGroupName")
        .with_column_of::<i64>("creationTime")
        .with_column_of::<i64>("storedBytes")
        .with_many("events", "logGroupName", log_events_table)
}

impl LogGroup {
    /// Pre-conditioned events table for *this* log group.
    ///
    /// Convenience wrapper around the same idea `with_foreign("events")`
    /// expresses, but typed and per-record — useful when you already
    /// have a resolved entity instead of a narrowed source table.
    pub fn ref_events(&self, aws: AwsAccount) -> Table<AwsAccount, LogEvent> {
        let mut t = log_events_table(aws);
        t.add_condition(eq("logGroupName", self.log_group_name.clone()));
        t
    }
}
