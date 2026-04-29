//! Ready-made IAM tables.
//!
//! IAM speaks the older AWS Query protocol (form-encoded request,
//! XML response), which the [`crate::query`] module handles. From the
//! caller's perspective these are no different from the JSON-1.1
//! tables — same `AwsAccount`, same `eq` filters, same `with_many`
//! relations.
//!
//! Top-level tables:
//!   - [`users_table`]              — `ListUsers`
//!   - [`groups_table`]             — `ListGroups`
//!   - [`roles_table`]              — `ListRoles`
//!   - [`policies_table`]           — `ListPolicies`
//!   - [`access_keys_table`]        — `ListAccessKeys`  (per user)
//!   - [`instance_profiles_table`]  — `ListInstanceProfiles`
//!
//! Relations wired on the entity factories:
//!   - `User`  → `groups`, `access_keys`, `attached_policies`
//!   - `Group` → `attached_policies`
//!   - `Role`  → `attached_policies`, `instance_profiles`
//!
//! ```no_run
//! # use vantage_aws::AwsAccount;
//! # use vantage_aws::models::iam::users_table;
//! # use vantage_dataset::prelude::ReadableValueSet;
//! # async fn run() -> vantage_core::Result<()> {
//! # let aws = AwsAccount::from_default()?;
//! let users = users_table(aws);
//! for user in users.list_values().await? {
//!     // …
//! }
//! # Ok(()) }
//! ```

pub mod access_key;
pub mod attached_policy;
pub mod group;
pub mod instance_profile;
pub mod policy;
pub mod role;
pub mod user;

pub use access_key::{AccessKey, access_keys_table};
pub use attached_policy::{
    AttachedPolicy, attached_group_policies_table, attached_role_policies_table,
    attached_user_policies_table,
};
pub use group::{Group, groups_table};
pub use instance_profile::{InstanceProfile, instance_profiles_table};
pub use policy::{Policy, policies_table};
pub use role::{Role, roles_table};
pub use user::{User, users_table};
