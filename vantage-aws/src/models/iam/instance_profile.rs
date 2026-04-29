use serde::{Deserialize, Serialize};
use vantage_table::table::Table;

use crate::AwsAccount;

/// One IAM instance profile from `ListInstanceProfiles` /
/// `ListInstanceProfilesForRole`. The nested `Roles` array isn't
/// surfaced as a typed field — XML payloads collapse to a single
/// string here in v0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProfile {
    #[serde(rename = "InstanceProfileName")]
    pub instance_profile_name: String,
    #[serde(rename = "InstanceProfileId", default)]
    pub instance_profile_id: String,
    #[serde(rename = "Arn", default)]
    pub arn: String,
    #[serde(rename = "Path", default)]
    pub path: String,
    #[serde(rename = "CreateDate", default)]
    pub create_date: String,
}

/// `ListInstanceProfiles` table — every instance profile in the
/// account. Optional filter: `PathPrefix`.
pub fn instance_profiles_table(aws: AwsAccount) -> Table<AwsAccount, InstanceProfile> {
    Table::new(
        "query/InstanceProfiles:iam/2010-05-08.ListInstanceProfiles",
        aws,
    )
    .with_id_column("InstanceProfileName")
    .with_column_of::<String>("InstanceProfileId")
    .with_column_of::<String>("Arn")
    .with_column_of::<String>("Path")
    .with_column_of::<String>("CreateDate")
}

/// `ListInstanceProfilesForRole` table — instance profiles that wrap
/// a given role. Requires `eq("RoleName", "...")`. Used as the
/// `instance_profiles` relation on `Role`.
pub(crate) fn instance_profiles_for_role_table(
    aws: AwsAccount,
) -> Table<AwsAccount, InstanceProfile> {
    Table::new(
        "query/InstanceProfiles:iam/2010-05-08.ListInstanceProfilesForRole",
        aws,
    )
    .with_id_column("InstanceProfileName")
    .with_column_of::<String>("InstanceProfileId")
    .with_column_of::<String>("Arn")
    .with_column_of::<String>("Path")
    .with_column_of::<String>("CreateDate")
}
