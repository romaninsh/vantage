use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One Lambda alias from `ListAliases`. Field names match the wire
/// JSON; v0 surfaces them flat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alias {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "AliasArn", default)]
    pub alias_arn: String,
    #[serde(rename = "FunctionVersion", default)]
    pub function_version: String,
    #[serde(rename = "Description", default)]
    pub description: String,
    #[serde(rename = "RevisionId", default)]
    pub revision_id: String,
}

/// `ListAliases` table. Requires `eq("FunctionName", "...")` — without
/// it the `{FunctionName}` path placeholder errors at request-build
/// time. Used as the `aliases` relation on [`super::function::Function`].
pub fn aliases_table(aws: AwsAccount) -> Table<AwsAccount, Alias> {
    Table::new(
        "restjson/Aliases:lambda/GET /2015-03-31/functions/{FunctionName}/aliases",
        aws,
    )
    .with_id_column("Name")
    .with_title_column_of::<String>("FunctionVersion")
    .with_column_of::<String>("AliasArn")
    .with_column_of::<String>("Description")
    .with_column_of::<String>("RevisionId")
}
