use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::types::{Arn, AwsDateTime};
use crate::{AwsAccount, eq};

/// One IAM managed policy from `ListPolicies`. Numeric fields stay as
/// strings — the Query protocol's XML response is untyped, and we
/// don't coerce in v0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(rename = "PolicyName")]
    pub policy_name: String,
    #[serde(rename = "PolicyId", default)]
    pub policy_id: String,
    #[serde(rename = "Arn", default)]
    pub arn: String,
    #[serde(rename = "Path", default)]
    pub path: String,
    #[serde(rename = "DefaultVersionId", default)]
    pub default_version_id: String,
    #[serde(rename = "AttachmentCount", default)]
    pub attachment_count: String,
    #[serde(rename = "PermissionsBoundaryUsageCount", default)]
    pub permissions_boundary_usage_count: String,
    #[serde(rename = "IsAttachable", default)]
    pub is_attachable: String,
    #[serde(rename = "CreateDate", default)]
    pub create_date: String,
    #[serde(rename = "UpdateDate", default)]
    pub update_date: String,
    #[serde(rename = "Description", default)]
    pub description: String,
}

/// `ListPolicies` table — managed policies in the account. Useful
/// filters: `Scope` (`AWS` / `Local` / `All`), `OnlyAttached` (`true`
/// to skip dormant policies), `PathPrefix`, `PolicyUsageFilter`
/// (`PermissionsPolicy` / `PermissionsBoundary`).
///
/// ```no_run
/// # use vantage_aws::{AwsAccount, eq};
/// # use vantage_aws::models::iam::policies_table;
/// # async fn run() -> vantage_core::Result<()> {
/// # let aws = AwsAccount::from_default()?;
/// let mut customer = policies_table(aws);
/// customer.add_condition(eq("Scope", "Local"));
/// # Ok(()) }
/// ```
pub fn policies_table(aws: AwsAccount) -> Table<AwsAccount, Policy> {
    Table::new("query/Policies:iam/2010-05-08.ListPolicies", aws)
        .with_id_column("PolicyName")
        .with_column_of::<String>("PolicyId")
        .with_column_of::<Arn>("Arn")
        .with_title_column_of::<String>("Path")
        .with_column_of::<String>("DefaultVersionId")
        .with_title_column_of::<i64>("AttachmentCount")
        .with_column_of::<i64>("PermissionsBoundaryUsageCount")
        .with_title_column_of::<bool>("IsAttachable")
        .with_column_of::<AwsDateTime>("CreateDate")
        .with_column_of::<AwsDateTime>("UpdateDate")
        .with_column_of::<String>("Description")
}

impl Policy {
    /// Build a [`policies_table`] narrowed to the policy named in
    /// `arn`. Accepts both AWS-managed (`arn:aws:iam::aws:policy/...`)
    /// and customer-managed (`arn:aws:iam::<account>:policy/...`) ARNs.
    pub fn from_arn(arn: &str, aws: AwsAccount) -> Option<Table<AwsAccount, Policy>> {
        let after = arn
            .strip_prefix("arn:aws:iam::")?
            .split(":policy/")
            .nth(1)?;
        // Customer-managed policies can sit under a path; managed
        // policies don't. Either way, the policy name is the last
        // path component.
        let name = after.rsplit('/').next()?;
        if name.is_empty() {
            return None;
        }
        let mut t = policies_table(aws);
        t.add_condition(eq("PolicyName", name.to_string()));
        Some(t)
    }
}
