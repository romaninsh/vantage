//! "Attached managed policy" reference returned by
//! `ListAttachedUserPolicies` / `ListAttachedGroupPolicies` /
//! `ListAttachedRolePolicies`. All three actions share the same
//! response shape — an `AttachedPolicies` array of `{PolicyName,
//! PolicyArn}` — so we model a single struct and expose three
//! factories that differ only in the table name (= the action).

use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One row of an attached-policies listing. The full policy document
/// isn't returned — fetch it separately via `Policy` if needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachedPolicy {
    #[serde(rename = "PolicyName")]
    pub policy_name: String,
    #[serde(rename = "PolicyArn", default)]
    pub policy_arn: String,
}

/// `ListAttachedUserPolicies` — requires `eq("UserName", "...")`.
/// Used as the `attached_policies` relation on `User`.
pub fn attached_user_policies_table(aws: AwsAccount) -> Table<AwsAccount, AttachedPolicy> {
    Table::new(
        "query/AttachedPolicies:iam/2010-05-08.ListAttachedUserPolicies",
        aws,
    )
    .with_id_column("PolicyArn")
    .with_title_column_of::<String>("PolicyName")
}

/// `ListAttachedGroupPolicies` — requires `eq("GroupName", "...")`.
/// Used as the `attached_policies` relation on `Group`.
pub fn attached_group_policies_table(aws: AwsAccount) -> Table<AwsAccount, AttachedPolicy> {
    Table::new(
        "query/AttachedPolicies:iam/2010-05-08.ListAttachedGroupPolicies",
        aws,
    )
    .with_id_column("PolicyArn")
    .with_title_column_of::<String>("PolicyName")
}

/// `ListAttachedRolePolicies` — requires `eq("RoleName", "...")`.
/// Used as the `attached_policies` relation on `Role`.
pub fn attached_role_policies_table(aws: AwsAccount) -> Table<AwsAccount, AttachedPolicy> {
    Table::new(
        "query/AttachedPolicies:iam/2010-05-08.ListAttachedRolePolicies",
        aws,
    )
    .with_id_column("PolicyArn")
    .with_title_column_of::<String>("PolicyName")
}
