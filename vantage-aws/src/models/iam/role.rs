use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::{AwsAccount, eq};

use super::attached_policy::{AttachedPolicy, attached_role_policies_table};
use super::instance_profile::{InstanceProfile, instance_profiles_for_role_table};

/// One IAM role from `ListRoles`. The trust policy
/// (`AssumeRolePolicyDocument`) is URL-encoded JSON when AWS returns
/// it — we surface it raw; decoding is the caller's call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    #[serde(rename = "RoleName")]
    pub role_name: String,
    #[serde(rename = "RoleId", default)]
    pub role_id: String,
    #[serde(rename = "Arn", default)]
    pub arn: String,
    #[serde(rename = "Path", default)]
    pub path: String,
    #[serde(rename = "CreateDate", default)]
    pub create_date: String,
    #[serde(rename = "Description", default)]
    pub description: String,
    #[serde(rename = "AssumeRolePolicyDocument", default)]
    pub assume_role_policy_document: String,
    #[serde(rename = "MaxSessionDuration", default)]
    pub max_session_duration: String,
}

/// `ListRoles` table — every IAM role in the account. Optional
/// filter: `PathPrefix`.
///
/// Two relations:
///   - `attached_policies` → `ListAttachedRolePolicies` for this role
///   - `instance_profiles` → `ListInstanceProfilesForRole` for this role
///
/// ```no_run
/// # use vantage_aws::{AwsAccount, eq};
/// # use vantage_aws::models::iam::roles_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let mut roles = roles_table(aws);
/// roles.add_condition(eq("PathPrefix", "/service-role/"));
/// # Ok(()) }
/// ```
pub fn roles_table(aws: AwsAccount) -> Table<AwsAccount, Role> {
    Table::new("query/Roles:iam/2010-05-08.ListRoles", aws)
        .with_id_column("RoleName")
        .with_column_of::<String>("RoleId")
        .with_column_of::<String>("Arn")
        .with_column_of::<String>("Path")
        .with_column_of::<String>("CreateDate")
        .with_column_of::<String>("Description")
        .with_column_of::<String>("AssumeRolePolicyDocument")
        .with_column_of::<String>("MaxSessionDuration")
        .with_many(
            "attached_policies",
            "RoleName",
            attached_role_policies_table,
        )
        .with_many(
            "instance_profiles",
            "RoleName",
            instance_profiles_for_role_table,
        )
}

impl Role {
    /// Attached managed policies for *this* role.
    pub fn ref_attached_policies(&self, aws: AwsAccount) -> Table<AwsAccount, AttachedPolicy> {
        let mut t = attached_role_policies_table(aws);
        t.add_condition(eq("RoleName", self.role_name.clone()));
        t
    }

    /// Instance profiles wrapping *this* role.
    pub fn ref_instance_profiles(&self, aws: AwsAccount) -> Table<AwsAccount, InstanceProfile> {
        let mut t = instance_profiles_for_role_table(aws);
        t.add_condition(eq("RoleName", self.role_name.clone()));
        t
    }
}
