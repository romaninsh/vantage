use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::types::{Arn, AwsDateTime};
use crate::{AwsAccount, eq};

use super::access_key::{AccessKey, access_keys_table};
use super::attached_policy::{AttachedPolicy, attached_user_policies_table};
use super::group::{Group, groups_for_user_table};

/// One IAM user from `ListUsers`. Field names match the wire shape —
/// the Query protocol returns these as XML elements; we expose them
/// 1:1 so existing IAM docs translate directly.
///
/// Dates come through as the raw ISO-8601 string AWS sends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    #[serde(rename = "UserName")]
    pub user_name: String,
    #[serde(rename = "UserId", default)]
    pub user_id: String,
    #[serde(rename = "Arn", default)]
    pub arn: String,
    #[serde(rename = "Path", default)]
    pub path: String,
    #[serde(rename = "CreateDate", default)]
    pub create_date: String,
    #[serde(rename = "PasswordLastUsed", default)]
    pub password_last_used: String,
}

/// `ListUsers` table — every IAM user in the account. Optional
/// filters: `PathPrefix` to narrow by path, `MaxItems` to cap the
/// page (v0 still only reads the first page either way).
///
/// Three relations:
///   - `groups` → `ListGroupsForUser` for this user
///   - `access_keys` → `ListAccessKeys` for this user
///   - `attached_policies` → `ListAttachedUserPolicies` for this user
///
/// AWS doesn't accept multi-value filters, so the source has to
/// narrow to a single user before traversal — otherwise the call
/// errors at execute time.
///
/// ```no_run
/// # use vantage_aws::{AwsAccount, eq};
/// # use vantage_aws::models::iam::users_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let mut users = users_table(aws);
/// users.add_condition(eq("PathPrefix", "/admin/"));
/// # Ok(()) }
/// ```
pub fn users_table(aws: AwsAccount) -> Table<AwsAccount, User> {
    Table::new("query/Users:iam/2010-05-08.ListUsers", aws)
        .with_id_column("UserName")
        .with_column_of::<String>("UserId")
        .with_column_of::<Arn>("Arn")
        .with_title_column_of::<String>("Path")
        .with_title_column_of::<AwsDateTime>("CreateDate")
        .with_column_of::<AwsDateTime>("PasswordLastUsed")
        .with_many("groups", "UserName", groups_for_user_table)
        .with_many("access_keys", "UserName", access_keys_table)
        .with_many(
            "attached_policies",
            "UserName",
            attached_user_policies_table,
        )
}

impl User {
    /// Build a [`users_table`] narrowed to the user named in `arn`.
    ///
    /// Accepts ARNs of the shape
    /// `arn:aws:iam::<account>:user/<name>`. Returns `None` if `arn`
    /// isn't an IAM-user ARN.
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, User>> {
        let name = arn.strip_prefix("arn:aws:iam::")?.split(":user/").nth(1)?;
        if name.is_empty() {
            return None;
        }
        let mut t = users_table(aws);
        t.add_condition(eq("UserName", name.to_string()));
        Some(t)
    }

    /// Groups *this* user belongs to.
    pub fn ref_groups(&self, aws: AwsAccount) -> Table<AwsAccount, Group> {
        let mut t = groups_for_user_table(aws);
        t.add_condition(eq("UserName", self.user_name.clone()));
        t
    }

    /// Access keys for *this* user.
    pub fn ref_access_keys(&self, aws: AwsAccount) -> Table<AwsAccount, AccessKey> {
        let mut t = access_keys_table(aws);
        t.add_condition(eq("UserName", self.user_name.clone()));
        t
    }

    /// Attached managed policies for *this* user.
    pub fn ref_attached_policies(&self, aws: AwsAccount) -> Table<AwsAccount, AttachedPolicy> {
        let mut t = attached_user_policies_table(aws);
        t.add_condition(eq("UserName", self.user_name.clone()));
        t
    }
}
