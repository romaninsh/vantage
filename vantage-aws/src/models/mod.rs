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
//! S3 (REST-XML, under [`s3`]):
//!   - [`s3::buckets_table`] — `ListBuckets`
//!   - [`s3::objects_table`] — `ListObjectsV2` (per bucket)
//!
//! Lambda (REST-JSON, under [`lambda`]):
//!   - [`lambda::functions_table`] — `ListFunctions`
//!   - [`lambda::aliases_table`]   — `ListAliases` (per function)
//!   - [`lambda::versions_table`]  — `ListVersionsByFunction` (per function)
//!
//! DynamoDB (JSON-1.0, under [`dynamodb`]):
//!   - [`dynamodb::tables_table`] — `ListTables`
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

pub mod dynamodb;
pub mod ecs;
pub mod iam;
pub mod lambda;
pub mod logs;
pub mod s3;

use vantage_vista::Vista;

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
    ///   - `s3.bucket ... :objects`           (ListObjectsV2 needs Bucket)
    ///   - `lambda.function ... :aliases`     (ListAliases needs FunctionName)
    ///   - `lambda.function ... :versions`    (ListVersionsByFunction needs FunctionName)
    ///   - `lambda.function ... :log_group`   (CloudWatch group at /aws/lambda/<name>)
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
            "s3.bucket",
            "s3.buckets",
            "lambda.function",
            "lambda.functions",
            "dynamodb.table",
            "dynamodb.tables",
        ]
    }

    /// Resolve a model name to a fully-constructed [`Vista`] plus its
    /// natural mode (singular → [`FactoryMode::Single`], plural →
    /// [`FactoryMode::List`]).
    ///
    /// Composite-id endpoints (`iam.access_keys`, `s3.objects`,
    /// `lambda.aliases`, `lambda.versions`, `ecs.services`, `ecs.tasks`,
    /// `log.streams`, `log.events`) intentionally aren't surfaced here:
    /// the AWS list endpoint requires a parent filter, so the only
    /// useful way to reach them is via `:relation` traversal from the
    /// parent.
    pub fn for_name(&self, name: &str) -> Option<(Vista, FactoryMode)> {
        let aws = self.aws.clone();
        let factory = aws.vista_factory();
        let (vista, mode) = match name {
            "iam.user" => (
                factory.from_table(iam::users_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "iam.users" => (
                factory.from_table(iam::users_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "iam.group" => (
                factory.from_table(iam::groups_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "iam.groups" => (
                factory.from_table(iam::groups_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "iam.role" => (
                factory.from_table(iam::roles_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "iam.roles" => (
                factory.from_table(iam::roles_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "iam.policy" => (
                factory.from_table(iam::policies_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "iam.policies" => (
                factory.from_table(iam::policies_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "iam.instance_profile" => (
                factory.from_table(iam::instance_profiles_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "iam.instance_profiles" => (
                factory.from_table(iam::instance_profiles_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "log.group" => (
                factory.from_table(logs::groups_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "log.groups" => (
                factory.from_table(logs::groups_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "ecs.cluster" => (
                factory.from_table(ecs::clusters_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "ecs.clusters" => (
                factory.from_table(ecs::clusters_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "ecs.task_definition" => (
                factory.from_table(ecs::task_definitions_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "ecs.task_definitions" => (
                factory.from_table(ecs::task_definitions_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "s3.bucket" => (
                factory.from_table(s3::buckets_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "s3.buckets" => (
                factory.from_table(s3::buckets_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "lambda.function" => (
                factory.from_table(lambda::functions_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "lambda.functions" => (
                factory.from_table(lambda::functions_table(aws)).ok()?,
                FactoryMode::List,
            ),
            "dynamodb.table" => (
                factory.from_table(dynamodb::tables_table(aws)).ok()?,
                FactoryMode::Single,
            ),
            "dynamodb.tables" => (
                factory.from_table(dynamodb::tables_table(aws)).ok()?,
                FactoryMode::List,
            ),
            _ => return None,
        };
        Some((vista, mode))
    }

    /// Resolve an ARN to a pre-conditioned single-record [`Vista`].
    /// Returns `None` if no entity recognises the ARN's resource type.
    ///
    /// Dispatch order: each entity's `from_arn` runs in turn — S3 object
    /// ARNs (`arn:aws:s3:::bucket/key`) are probed before bucket ARNs
    /// since the object form is a strict superset.
    pub fn from_arn(&self, arn: &str) -> Option<Vista> {
        let aws = self.aws.clone();
        let factory = aws.vista_factory();
        if let Some(t) = iam::user::User::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = iam::group::Group::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = iam::role::Role::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = iam::policy::Policy::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = iam::instance_profile::InstanceProfile::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = iam::access_key::AccessKey::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = logs::stream::LogStream::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = logs::group::LogGroup::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = ecs::cluster::Cluster::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = s3::object::Object::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = s3::bucket::Bucket::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = lambda::function::Function::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        if let Some(t) = dynamodb::table::DynamoDbTable::from_arn(arn, aws.clone()) {
            return factory.from_table(t).ok();
        }
        None
    }
}
