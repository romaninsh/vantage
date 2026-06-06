//! `CmdModelFactory` — maps dotted model names (`iam.users`, `log.groups`,
//! `log.group`, …) to YAML-backed [`Vista`]s, mirroring
//! `vantage_aws::models::Factory`.
//!
//! The YAML vistas are bundled with the crate and used as the `aws-cli`
//! example's model set. Add a model by dropping a `*.yaml` in `vistas/`
//! and adding it to [`CmdModelFactory::yaml_for`] / [`known_names`].

use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{ReferenceKind, Vista};

use crate::cmd::Cmd;
use crate::vista::spec::CmdVistaSpec;

const IAM_USERS: &str = include_str!("../../vistas/iam.users.yaml");
const IAM_GROUPS: &str = include_str!("../../vistas/iam.groups.yaml");
const IAM_ROLES: &str = include_str!("../../vistas/iam.roles.yaml");
const IAM_POLICIES: &str = include_str!("../../vistas/iam.policies.yaml");
const IAM_USER_GROUPS: &str = include_str!("../../vistas/iam.user_groups.yaml");
const IAM_ACCESS_KEYS: &str = include_str!("../../vistas/iam.access_keys.yaml");
const IAM_USER_POLICIES: &str = include_str!("../../vistas/iam.user_policies.yaml");
const LOG_GROUPS: &str = include_str!("../../vistas/log.groups.yaml");
const LOG_STREAMS: &str = include_str!("../../vistas/log.streams.yaml");
const LOG_EVENTS: &str = include_str!("../../vistas/log.events.yaml");
const ECS_CLUSTERS: &str = include_str!("../../vistas/ecs.clusters.yaml");
const ECS_SERVICES: &str = include_str!("../../vistas/ecs.services.yaml");
const ECS_TASKS: &str = include_str!("../../vistas/ecs.tasks.yaml");
const ECS_TASK_DEFINITIONS: &str = include_str!("../../vistas/ecs.task_definitions.yaml");
const S3_BUCKETS: &str = include_str!("../../vistas/s3.buckets.yaml");
const S3_OBJECTS: &str = include_str!("../../vistas/s3.objects.yaml");
const LAMBDA_FUNCTIONS: &str = include_str!("../../vistas/lambda.functions.yaml");
const LAMBDA_ALIASES: &str = include_str!("../../vistas/lambda.aliases.yaml");
const LAMBDA_VERSIONS: &str = include_str!("../../vistas/lambda.versions.yaml");

/// Whether a lookup is naturally a list (plural name) or a single record
/// (singular name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactoryMode {
    List,
    Single,
}

/// Builds vistas by name off a shared [`Cmd`].
#[derive(Clone)]
pub struct CmdModelFactory {
    cmd: Cmd,
}

impl CmdModelFactory {
    pub fn new(cmd: Cmd) -> Self {
        Self { cmd }
    }

    /// Top-level model names exposed to a CLI.
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
        ]
    }

    /// The YAML for a model name (singular and plural map to the same spec).
    /// Relation targets (`log.streams`, `log.events`) resolve here too even
    /// though they aren't top-level `known_names`.
    fn yaml_for(name: &str) -> Option<&'static str> {
        Some(match name {
            "iam.user" | "iam.users" => IAM_USERS,
            "iam.group" | "iam.groups" => IAM_GROUPS,
            "iam.role" | "iam.roles" => IAM_ROLES,
            "iam.policy" | "iam.policies" => IAM_POLICIES,
            // Relation targets (reached via `:groups` / `:access_keys` /
            // `:policies` from iam.user); not surfaced top-level.
            "iam.user_groups" => IAM_USER_GROUPS,
            "iam.access_keys" => IAM_ACCESS_KEYS,
            "iam.user_policies" => IAM_USER_POLICIES,
            "log.group" | "log.groups" => LOG_GROUPS,
            "log.stream" | "log.streams" => LOG_STREAMS,
            "log.event" | "log.events" => LOG_EVENTS,
            "ecs.cluster" | "ecs.clusters" => ECS_CLUSTERS,
            "ecs.service" | "ecs.services" => ECS_SERVICES,
            "ecs.task" | "ecs.tasks" => ECS_TASKS,
            "ecs.task_definition" | "ecs.task_definitions" => ECS_TASK_DEFINITIONS,
            "s3.bucket" | "s3.buckets" => S3_BUCKETS,
            "s3.object" | "s3.objects" => S3_OBJECTS,
            "lambda.function" | "lambda.functions" => LAMBDA_FUNCTIONS,
            "lambda.alias" | "lambda.aliases" => LAMBDA_ALIASES,
            "lambda.version" | "lambda.versions" => LAMBDA_VERSIONS,
            _ => return None,
        })
    }

    fn is_singular(name: &str) -> bool {
        matches!(
            name,
            "iam.user"
                | "iam.group"
                | "iam.role"
                | "iam.policy"
                | "log.group"
                | "log.stream"
                | "log.event"
                | "ecs.cluster"
                | "ecs.service"
                | "ecs.task"
                | "ecs.task_definition"
                | "s3.bucket"
                | "s3.object"
                | "lambda.function"
                | "lambda.alias"
                | "lambda.version"
        )
    }

    /// Resolve a model name to a `Vista` plus its natural mode.
    ///
    /// The vista's references are lowered onto the wrapped `Table<Cmd>` (see
    /// [`build_table`](Self::build_table)), so the CLI's `:relation`
    /// traversal flows through the built-in `Table::get_ref_from_row` path.
    pub fn for_name(&self, name: &str) -> Option<(Vista, FactoryMode)> {
        let table = Self::build_table(self.cmd.clone(), name)?;
        let vista = self.cmd.vista_factory().from_table(table).ok()?;
        let mode = if Self::is_singular(name) {
            FactoryMode::Single
        } else {
            FactoryMode::List
        };
        Some((vista, mode))
    }

    /// Build a fully-referenced `Table<Cmd>` for a model from its bundled
    /// YAML. Columns / id come from [`CmdVistaFactory::build_columns_table`];
    /// each YAML `references:` entry becomes a real `with_many` / `with_one`
    /// registration whose target is resolved (lazily, on traversal) by name
    /// through this same builder.
    fn build_table(cmd: Cmd, name: &str) -> Option<Table<Cmd, EmptyEntity>> {
        let yaml = Self::yaml_for(name)?;
        let spec: CmdVistaSpec = serde_yaml_ng::from_str(yaml).ok()?;
        let mut table = cmd.vista_factory().build_columns_table(&spec).ok()?;

        for (rel_name, ref_spec) in &spec.references {
            let target = ref_spec.table.clone();
            let fk = ref_spec
                .foreign_key
                .clone()
                .unwrap_or_else(|| rel_name.clone());
            // The target is bundled YAML, so a resolve failure is a build-time
            // bug, not a runtime condition — surface it loudly.
            let build_target = move |cmd: Cmd| {
                Self::build_table(cmd, &target)
                    .expect("bundled vista reference target must resolve")
            };
            table = match ref_spec.kind {
                ReferenceKind::HasMany => table.with_many(rel_name, &fk, build_target),
                ReferenceKind::HasOne => table.with_one(rel_name, &fk, build_target),
            };
        }

        Some(table)
    }
}
