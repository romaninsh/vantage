//! Ready-made tables to skip the table-name dance.
//!
//! CloudWatch Logs (JSON-1.1, under [`logs`]):
//!   - [`logs::groups_table`]  — `DescribeLogGroups`
//!   - [`logs::streams_table`] — `DescribeLogStreams`
//!   - [`logs::events_table`]  — `FilterLogEvents`
//!
//! ECS (JSON-1.1, under [`ecs`]):
//!   - [`ecs::clusters_table`]
//!   - [`ecs::services_table`]
//!   - [`ecs::tasks_table`]
//!   - [`ecs::task_definitions_table`]
//!
//! IAM (Query, under [`iam`]):
//!   - [`iam::users_table`]              — `ListUsers`
//!   - [`iam::groups_table`]             — `ListGroups`
//!   - [`iam::roles_table`]              — `ListRoles`
//!   - [`iam::policies_table`]           — `ListPolicies`
//!   - [`iam::access_keys_table`]        — `ListAccessKeys`  (per user)
//!   - [`iam::instance_profiles_table`]  — `ListInstanceProfiles`
//!
//! ## Generic factory ([`Factory`])
//!
//! Wraps every table above behind dotted-string names (`iam.users`,
//! `log.group`, `ecs.task_definitions`, …) and a single
//! [`Factory::from_arn`] entry point. Powers the `aws-cli` example
//! (which adapts it to `vantage_cli_util`'s `ModelFactory` trait);
//! anything else that needs a generic, type-erased AWS table by name
//! can reuse it without dragging in a CLI rendering crate.
//!
//! ```no_run
//! # use vantage_aws::{AwsAccount, eq};
//! # use vantage_aws::models::logs::groups_table;
//! # async fn run() -> vantage_core::Result<()> {
//! let aws = AwsAccount::from_default()?;
//! let mut groups = groups_table(aws);
//! groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
//! # Ok(()) }
//! ```

pub mod ecs;
pub mod iam;
pub mod logs;

use vantage_table::any::AnyTable;

use crate::AwsAccount;

/// Whether a [`Factory`] lookup should drop into list mode (returning
/// every matching record) or single-record mode (returning just the
/// first match).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactoryMode {
    List,
    Single,
}

/// Generic, type-erased model factory.
///
/// The factory maps dotted string names to the typed `*_table`
/// factories above and dispatches ARN parsing across each entity's
/// `from_arn`. Singular forms (e.g. `iam.user`) drop into
/// [`FactoryMode::Single`]; plural forms (`iam.users`) drop into
/// [`FactoryMode::List`].
#[derive(Debug, Clone)]
pub struct Factory {
    aws: AwsAccount,
}

impl Factory {
    /// Build a factory bound to a specific AWS account.
    pub fn new(aws: AwsAccount) -> Self {
        Self { aws }
    }

    /// All known model names, in registration order.
    ///
    /// Models whose AWS API requires a parent filter aren't exposed
    /// top-level — listing them standalone would either error
    /// or quietly return only the caller's slice. Reach them via
    /// traversal from their parent:
    ///   - `iam.user ... :access_keys`        (ListAccessKeys needs UserName)
    ///   - `log.group ... :streams`           (DescribeLogStreams needs logGroupName)
    ///   - `log.group ... :events`            (FilterLogEvents needs logGroupName)
    ///   - `ecs.cluster ... :services`        (ListServices needs cluster)
    ///   - `ecs.cluster ... :tasks`           (ListTasks needs cluster)
    ///
    /// Per-resource ARNs still work as the first argument for any of
    /// these — see [`Factory::from_arn`].
    pub fn known_names() -> &'static [&'static str] {
        &[
            "iam.user",
            "iam.users",
            "iam.group",
            "iam.groups",
            "iam.role",
            "iam.roles",
            "iam.policy",
            "iam.policies",
            "iam.instance_profile",
            "iam.instance_profiles",
            "log.group",
            "log.groups",
            "ecs.cluster",
            "ecs.clusters",
            "ecs.task_definition",
            "ecs.task_definitions",
        ]
    }

    /// Resolve a model name to an `AnyTable` plus its mode.
    pub fn for_name(&self, name: &str) -> Option<(AnyTable, FactoryMode)> {
        let aws = self.aws.clone();
        let (table, mode) = match name {
            "iam.user" => (AnyTable::new(iam::users_table(aws)), FactoryMode::Single),
            "iam.users" => (AnyTable::new(iam::users_table(aws)), FactoryMode::List),
            "iam.group" => (AnyTable::new(iam::groups_table(aws)), FactoryMode::Single),
            "iam.groups" => (AnyTable::new(iam::groups_table(aws)), FactoryMode::List),
            "iam.role" => (AnyTable::new(iam::roles_table(aws)), FactoryMode::Single),
            "iam.roles" => (AnyTable::new(iam::roles_table(aws)), FactoryMode::List),
            "iam.policy" => (AnyTable::new(iam::policies_table(aws)), FactoryMode::Single),
            "iam.policies" => (AnyTable::new(iam::policies_table(aws)), FactoryMode::List),
            // iam.access_key / iam.access_keys intentionally omitted:
            // listing them standalone returns just the caller's keys,
            // which is rarely what people mean. Reach them via
            // `iam.user ... :access_keys`.
            "iam.instance_profile" => (
                AnyTable::new(iam::instance_profiles_table(aws)),
                FactoryMode::Single,
            ),
            "iam.instance_profiles" => (
                AnyTable::new(iam::instance_profiles_table(aws)),
                FactoryMode::List,
            ),
            "log.group" => (AnyTable::new(logs::groups_table(aws)), FactoryMode::Single),
            "log.groups" => (AnyTable::new(logs::groups_table(aws)), FactoryMode::List),
            // log.stream / log.event intentionally omitted: AWS
            // requires `logGroupName`. Reach them via
            // `log.group ... :streams` / `:events`.
            "ecs.cluster" => (AnyTable::new(ecs::clusters_table(aws)), FactoryMode::Single),
            "ecs.clusters" => (AnyTable::new(ecs::clusters_table(aws)), FactoryMode::List),
            // ecs.service / ecs.task intentionally omitted: AWS
            // requires `cluster` as a filter, so listing them
            // standalone returns nothing useful. Reach them via
            // `ecs.cluster ... :services` / `:tasks`.
            "ecs.task_definition" => (
                AnyTable::new(ecs::task_definitions_table(aws)),
                FactoryMode::Single,
            ),
            "ecs.task_definitions" => (
                AnyTable::new(ecs::task_definitions_table(aws)),
                FactoryMode::List,
            ),
            _ => return None,
        };
        Some((table, mode))
    }

    /// Resolve an ARN to a pre-conditioned single-record table by
    /// dispatching to each entity's `from_arn`. Returns `None` if no
    /// entity recognises the ARN's resource type.
    pub fn from_arn(&self, arn: &str) -> Option<AnyTable> {
        let aws = self.aws.clone();
        if let Some(t) = iam::user::User::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = iam::group::Group::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = iam::role::Role::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = iam::policy::Policy::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = iam::instance_profile::InstanceProfile::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = iam::access_key::AccessKey::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = logs::stream::LogStream::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = logs::group::LogGroup::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        if let Some(t) = ecs::cluster::Cluster::from_arn(arn, aws.clone()) {
            return Some(AnyTable::new(t));
        }
        None
    }
}
