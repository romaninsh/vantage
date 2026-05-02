use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

/// One DynamoDB table from `ListTables`. The list-side response is
/// minimal — just an array of strings — so v0 surfaces the name only.
/// `DescribeTable` (single-record, richer metadata) is a v0+ feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamoDbTable {
    #[serde(rename = "TableName")]
    pub table_name: String,
}

/// `ListTables` table — every DynamoDB table in the account/region.
/// Use `eq("Limit", N)` if you need to cap the page; pagination via
/// `ExclusiveStartTableName` isn't surfaced in v0.
///
/// ```no_run
/// # use vantage_aws::AwsAccount;
/// # use vantage_aws::models::dynamodb::tables_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let tables = tables_table(aws);
/// # Ok(()) }
/// ```
pub fn tables_table(aws: AwsAccount) -> Table<AwsAccount, DynamoDbTable> {
    Table::new(
        "json10/TableNames:dynamodb/DynamoDB_20120810.ListTables",
        aws,
    )
    .with_id_column("TableName")
}

impl DynamoDbTable {
    /// Build a [`tables_table`] narrowed to the table named in `arn`.
    /// Accepts ARNs of the shape
    /// `arn:aws:dynamodb:<region>:<account>:table/<name>`.
    ///
    /// `ListTables` doesn't take a name filter, so the narrowing
    /// happens post-hoc client-side via the standard
    /// `impls::table_source` retain pass.
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, DynamoDbTable>> {
        let after = arn.split(":table/").nth(1)?;
        let name = after.split('/').next().unwrap_or(after);
        if name.is_empty() {
            return None;
        }
        let mut t = tables_table(aws);
        t.add_condition(eq("TableName", name.to_string()));
        Some(t)
    }
}
