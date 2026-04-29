use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::types::{Arn, AwsDateTime};
use crate::{AwsAccount, eq};

use super::attached_policy::{AttachedPolicy, attached_group_policies_table};

/// One IAM group from `ListGroups`. Same shape comes back from
/// `ListGroupsForUser`, which is why both factories below produce
/// `Table<AwsAccount, Group>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    #[serde(rename = "GroupName")]
    pub group_name: String,
    #[serde(rename = "GroupId", default)]
    pub group_id: String,
    #[serde(rename = "Arn", default)]
    pub arn: String,
    #[serde(rename = "Path", default)]
    pub path: String,
    #[serde(rename = "CreateDate", default)]
    pub create_date: String,
}

/// `ListGroups` table — every IAM group in the account. Optional
/// filter: `PathPrefix`.
///
/// Relation:
///   - `attached_policies` → `ListAttachedGroupPolicies` for this group
pub fn groups_table(aws: AwsAccount) -> Table<AwsAccount, Group> {
    Table::new("query/Groups:iam/2010-05-08.ListGroups", aws)
        .with_id_column("GroupName")
        .with_column_of::<String>("GroupId")
        .with_column_of::<Arn>("Arn")
        .with_column_of::<String>("Path")
        .with_column_of::<AwsDateTime>("CreateDate")
        .with_many(
            "attached_policies",
            "GroupName",
            attached_group_policies_table,
        )
}

/// `ListGroupsForUser` table — IAM groups that a given user belongs
/// to. Requires `eq("UserName", "...")` before listing. Used as the
/// `groups` relation on `User`; rarely interesting standalone.
pub(crate) fn groups_for_user_table(aws: AwsAccount) -> Table<AwsAccount, Group> {
    Table::new("query/Groups:iam/2010-05-08.ListGroupsForUser", aws)
        .with_id_column("GroupName")
        .with_column_of::<String>("GroupId")
        .with_column_of::<Arn>("Arn")
        .with_column_of::<String>("Path")
        .with_column_of::<AwsDateTime>("CreateDate")
}

impl Group {
    /// Attached managed policies for *this* group.
    pub fn ref_attached_policies(&self, aws: AwsAccount) -> Table<AwsAccount, AttachedPolicy> {
        let mut t = attached_group_policies_table(aws);
        t.add_condition(eq("GroupName", self.group_name.clone()));
        t
    }
}
