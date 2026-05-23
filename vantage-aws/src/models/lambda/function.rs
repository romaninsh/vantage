use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::types::{Arn, AwsDateTime};
use crate::{AwsAccount, eq};

use super::alias::{Alias, aliases_table};
use super::version::{Version, versions_table};
use crate::models::logs::group::{LogGroup, groups_table as log_groups_table};

/// One Lambda function from `ListFunctions`. Lambda's response
/// includes nested config blobs (`LoggingConfig`, `TracingConfig`,
/// `VpcConfig`, …) that v0 leaves nested as wire-shape — we surface
/// the scalars callers usually want at the top level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    #[serde(rename = "FunctionName")]
    pub function_name: String,
    #[serde(rename = "FunctionArn", default)]
    pub function_arn: String,
    #[serde(rename = "Runtime", default)]
    pub runtime: String,
    #[serde(rename = "Role", default)]
    pub role: String,
    #[serde(rename = "Handler", default)]
    pub handler: String,
    #[serde(rename = "Description", default)]
    pub description: String,
    #[serde(rename = "Timeout", default)]
    pub timeout: i64,
    #[serde(rename = "MemorySize", default)]
    pub memory_size: i64,
    #[serde(rename = "LastModified", default)]
    pub last_modified: String,
    #[serde(rename = "Version", default)]
    pub version: String,
    #[serde(rename = "PackageType", default)]
    pub package_type: String,
}

/// `ListFunctions` table — every function in the configured region.
/// `MasterRegion` and `FunctionVersion` are the only AWS-side filters;
/// most callers leave them off and rely on post-hoc narrowing.
///
/// Relations:
///   - `aliases` → `ListAliases` for this function
///   - `versions` → `ListVersionsByFunction` for this function
///
/// Cross-service traversal to CloudWatch Logs is available via the
/// inherent [`Function::ref_log_group`] helper. The typed `with_foreign`
/// machinery it previously used was removed in the Vista `get_ref`
/// rollout; wiring it as a cross-service Vista reference (via
/// `add_raw_condition` on the AWS shell) is deferred — for now,
/// reach the function's log group through the inherent helper.
///
/// ```no_run
/// # use vantage_aws::AwsAccount;
/// # use vantage_aws::models::lambda::functions_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let functions = functions_table(aws);
/// # Ok(()) }
/// ```
pub fn functions_table(aws: AwsAccount) -> Table<AwsAccount, Function> {
    Table::new("restjson/Functions:lambda/GET /2015-03-31/functions/", aws)
        .with_id_column("FunctionName")
        .with_column_of::<Arn>("FunctionArn")
        .with_title_column_of::<String>("Runtime")
        .with_title_column_of::<String>("Handler")
        .with_column_of::<String>("Description")
        .with_column_of::<Arn>("Role")
        .with_column_of::<i64>("Timeout")
        .with_column_of::<i64>("MemorySize")
        .with_column_of::<AwsDateTime>("LastModified")
        .with_column_of::<String>("Version")
        .with_column_of::<String>("PackageType")
        .with_many("aliases", "FunctionName", aliases_table)
        .with_many("versions", "FunctionName", versions_table)
}

impl Function {
    /// Build a [`functions_table`] narrowed to the function named in
    /// `arn`. Accepts ARNs of the shape
    /// `arn:aws:lambda:<region>:<account>:function:<name>` (with or
    /// without a trailing `:<qualifier>`).
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, Function>> {
        let after = arn.split(":function:").nth(1)?;
        // Strip optional version/alias qualifier.
        let name = after.split(':').next().unwrap_or(after);
        if name.is_empty() {
            return None;
        }
        let mut t = functions_table(aws);
        t.add_condition(eq("FunctionName", name.to_string()));
        Some(t)
    }

    /// Aliases for *this* function.
    pub fn ref_aliases(&self, aws: AwsAccount) -> Table<AwsAccount, Alias> {
        let mut t = aliases_table(aws);
        t.add_condition(eq("FunctionName", self.function_name.clone()));
        t
    }

    /// Published versions for *this* function (always includes
    /// `$LATEST`).
    pub fn ref_versions(&self, aws: AwsAccount) -> Table<AwsAccount, Version> {
        let mut t = versions_table(aws);
        t.add_condition(eq("FunctionName", self.function_name.clone()));
        t
    }

    /// CloudWatch Logs group for *this* function — `/aws/lambda/<name>`
    /// by default. Returns a [`crate::models::logs::group::LogGroup`]
    /// table pre-narrowed via `logGroupNamePrefix`.
    pub fn ref_log_group(&self, aws: AwsAccount) -> Table<AwsAccount, LogGroup> {
        let mut t = log_groups_table(aws);
        t.add_condition(eq(
            "logGroupNamePrefix",
            format!("/aws/lambda/{}", self.function_name),
        ));
        t
    }
}
