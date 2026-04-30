use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

/// One IAM access key from `ListAccessKeys`. Secret material is
/// never returned by the API — only the metadata you can use to
/// audit / rotate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKey {
    #[serde(rename = "AccessKeyId")]
    pub access_key_id: String,
    #[serde(rename = "UserName", default)]
    pub user_name: String,
    #[serde(rename = "Status", default)]
    pub status: String,
    #[serde(rename = "CreateDate", default)]
    pub create_date: String,
}

/// `ListAccessKeys` table — access keys for a single user. Filter by
/// `eq("UserName", "...")`; if omitted, AWS returns the keys for the
/// caller (`AwsAccount`).
///
/// Used as the `access_keys` relation on `User`.
pub fn access_keys_table(aws: AwsAccount) -> Table<AwsAccount, AccessKey> {
    Table::new("query/AccessKeyMetadata:iam/2010-05-08.ListAccessKeys", aws)
        .with_id_column("AccessKeyId")
        .with_title_column_of::<String>("UserName")
        .with_title_column_of::<String>("Status")
        .with_column_of::<String>("CreateDate")
}

impl AccessKey {
    /// Access keys don't have stable, addressable ARNs in IAM. The
    /// closest thing is the user-scoped form
    /// `arn:aws:iam::<account>:user/<name>/access-key/<id>` — accepted
    /// here as a convenience: we filter by `UserName` since that's the
    /// only condition `ListAccessKeys` accepts.
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, AccessKey>> {
        let after = arn.strip_prefix("arn:aws:iam::")?.split(":user/").nth(1)?;
        let user_name = after.split("/access-key/").next()?;
        if user_name.is_empty() {
            return None;
        }
        let mut t = access_keys_table(aws);
        t.add_condition(eq("UserName", user_name.to_string()));
        Some(t)
    }
}
