use serde::{Deserialize, Serialize};
use vantage_aws::{AwsAccount, AwsJson1, AwsOperation, eq};
use vantage_table::any::AnyTable;
use vantage_table::table::Table;

use crate::log_event::{LogEvent, log_events_table};

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
/// Registers `events` as a foreign relation: traversing it builds a
/// `FilterLogEvents` table whose `logGroupName` is bound to the source
/// table's `logGroupName` values via a deferred subquery. AWS rejects
/// multi-value filters, so the source must narrow to exactly one row
/// before traversal — otherwise the query errors at execute time.
pub fn log_groups_table(aws: AwsAccount) -> Table<AwsJson1, LogGroup> {
    let aws_for_events = aws.clone();
    Table::new(
        "logs/Logs_20140328.DescribeLogGroups",
        aws.json1("logGroups"),
    )
    .with_id_column("logGroupName")
    .with_column_of::<i64>("creationTime")
    .with_column_of::<i64>("storedBytes")
    .with_foreign("events", "Table<AwsJson1, LogEvent>", move |source| {
        let mut events = log_events_table(aws_for_events.clone());
        events.add_condition(events["logGroupName"].in_(source.column_values_expr("logGroupName")));
        Ok(AnyTable::new(events))
    })
}

impl LogGroup {
    /// Pre-conditioned events table for *this* log group.
    ///
    /// Convenience wrapper around the same idea `with_foreign("events")`
    /// expresses, but typed and per-record — useful when you already
    /// have a resolved entity instead of a narrowed source table.
    pub fn ref_events(&self, aws: AwsAccount) -> Table<AwsJson1, LogEvent> {
        let mut t = log_events_table(aws);
        t.add_condition(eq("logGroupName", self.log_group_name.clone()));
        t
    }
}
