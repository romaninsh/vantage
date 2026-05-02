use ciborium::Value as CborValue;
use serde::{Deserialize, Serialize};
use vantage_expressions::Expression;
use vantage_expressions::expr_any;
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};
use vantage_table::any::AnyTable;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;

use crate::condition::AwsCondition;
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
///   - `log_group` → CloudWatch Logs group at `/aws/lambda/<name>`
///     (cross-service into [`crate::models::logs`])
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
        .with_foreign(
            "log_group",
            std::any::type_name::<Table<AwsAccount, LogGroup>>(),
            log_group_relation,
        )
}

/// Build the `:log_group` traversal target for a Lambda function.
///
/// Standard `with_many` would project `FunctionName` straight into the
/// log-group filter, which doesn't match — log groups carry the
/// derived name `/aws/lambda/<FunctionName>`. So we splice that prefix
/// in here: a deferred expression runs the source query, pulls
/// `FunctionName` from each row, and prefixes it before handing it to
/// `logGroupNamePrefix` for AWS-side narrowing. The single-value
/// constraint that all `Deferred` conditions live under still applies
/// — multi-row sources error at execute time, same as any other
/// traversal.
fn log_group_relation(functions: &Table<AwsAccount, Function>) -> vantage_core::Result<AnyTable> {
    let aws = functions.data_source().clone();
    let mut groups = log_groups_table(aws.clone());

    let table = functions.clone();
    let aws_for_fn = aws.clone();
    let inner: DeferredFn<CborValue> = DeferredFn::new(move || {
        let aws = aws_for_fn.clone();
        let table = table.clone();
        Box::pin(async move {
            let records = aws.list_table_values(&table).await?;
            let names: Vec<CborValue> = records
                .values()
                .filter_map(|r| match r.get("FunctionName") {
                    Some(CborValue::Text(s)) => Some(CborValue::Text(format!("/aws/lambda/{s}"))),
                    _ => None,
                })
                .collect();
            Ok(ExpressiveEnum::Scalar(CborValue::Array(names)))
        })
    });

    let source: Expression<CborValue> = expr_any!("{}", { inner });

    groups.add_condition(AwsCondition::Deferred {
        field: "logGroupNamePrefix".to_string(),
        source,
    });

    Ok(AnyTable::from_table(groups))
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
