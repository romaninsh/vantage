use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One Lambda function version from `ListVersionsByFunction`. We keep
/// only the identifying fields here — the full FunctionConfiguration
/// is huge and rarely useful in a list view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "FunctionName", default)]
    pub function_name: String,
    #[serde(rename = "FunctionArn", default)]
    pub function_arn: String,
    #[serde(rename = "Runtime", default)]
    pub runtime: String,
    #[serde(rename = "LastModified", default)]
    pub last_modified: String,
    #[serde(rename = "Description", default)]
    pub description: String,
}

/// `ListVersionsByFunction` table. Requires `eq("FunctionName", "...")`.
/// Used as the `versions` relation on [`super::function::Function`].
///
/// Lambda always returns at least `$LATEST`; published versions show
/// up only after the function has been explicitly versioned.
pub fn versions_table(aws: AwsAccount) -> Table<AwsAccount, Version> {
    Table::new(
        "restjson/Versions:lambda/GET /2015-03-31/functions/{FunctionName}/versions",
        aws,
    )
    .with_id_column("Version")
    .with_title_column_of::<String>("Runtime")
    .with_column_of::<String>("FunctionArn")
    .with_column_of::<String>("LastModified")
    .with_column_of::<String>("Description")
}
