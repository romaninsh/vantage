//! `vantage-aws-cli` — proof-of-concept CLI driving the CloudWatch
//! Logs, ECS, and IAM models through `vantage-aws`. Renders results
//! via `vantage_cli_util::print_table` so the output exercises the
//! same `Table` / `TableSource` machinery the rest of the framework
//! uses.
//!
//! Reads creds from the standard env vars (`AWS_ACCESS_KEY_ID`,
//! `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, `AWS_REGION`),
//! falling back to the `[default]` profile in `~/.aws/credentials`
//! and `~/.aws/config`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use vantage_aws::models::ecs::{
    clusters_table, services_table, task_definitions_table, tasks_table,
};
use vantage_aws::models::iam::{
    Role, User, access_keys_table, groups_table as iam_groups_table, instance_profiles_table,
    policies_table, roles_table, users_table,
};
use vantage_aws::models::logs::{events_table, groups_table, streams_table};
use vantage_aws::{AwsAccount, eq};
use vantage_cli_util::{print_table, render_records};
use vantage_dataset::prelude::{ReadableDataSet, ReadableValueSet};

const TRAVERSE_GROUP: &str = "/ecs/ba-nginx";

#[derive(Parser)]
#[command(name = "vantage-aws-cli", about = "vantage-aws CloudWatch + ECS + IAM demo")]
struct Cli {
    /// Override the AWS region (sets AWS_REGION before credential load).
    #[arg(long, global = true)]
    region: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// CloudWatch: list log groups (optionally filtered by prefix).
    LogGroups {
        #[arg(long)]
        prefix: Option<String>,
    },
    /// CloudWatch: list log streams in a group.
    LogStreams { log_group_name: String },
    /// CloudWatch: list log events in a group.
    LogEvents { log_group_name: String },
    /// CloudWatch: filter to one group, drill into its events via `with_many("events")`.
    TraverseLogEvents,
    /// CloudWatch: filter to one group, drill into its streams via `with_many("streams")`.
    TraverseLogStreams,

    /// ECS: list clusters in the account.
    ListClusters,
    /// ECS: list services in a cluster (name or ARN).
    ListServices { cluster: String },
    /// ECS: list tasks in a cluster (name or ARN).
    ListTasks {
        cluster: String,
        /// Filter by service name.
        #[arg(long)]
        service: Option<String>,
        /// Filter by task definition family.
        #[arg(long)]
        family: Option<String>,
        /// RUNNING / PENDING / STOPPED.
        #[arg(long)]
        status: Option<String>,
    },
    /// ECS: list active task definitions (optionally filtered by family prefix).
    ListTaskDefs {
        #[arg(long)]
        family_prefix: Option<String>,
    },

    /// IAM: list users in the account.
    ListUsers {
        /// Filter by IAM path prefix.
        #[arg(long)]
        path_prefix: Option<String>,
    },
    /// IAM: list groups in the account.
    ListGroups {
        #[arg(long)]
        path_prefix: Option<String>,
    },
    /// IAM: list roles in the account.
    ListRoles {
        /// Filter by IAM path prefix (e.g. "/service-role/").
        #[arg(long)]
        path_prefix: Option<String>,
    },
    /// IAM: list managed policies in the account.
    ListPolicies {
        /// AWS / Local / All — defaults to All on the AWS side.
        #[arg(long)]
        scope: Option<String>,
        /// true to skip dormant policies.
        #[arg(long)]
        only_attached: Option<String>,
        #[arg(long)]
        path_prefix: Option<String>,
    },
    /// IAM: list access keys for a user (defaults to the caller).
    ListAccessKeys {
        #[arg(long)]
        user: Option<String>,
    },
    /// IAM: list instance profiles in the account.
    ListInstanceProfiles {
        #[arg(long)]
        path_prefix: Option<String>,
    },

    /// IAM: filter to one user, drill into their groups via `with_many("groups")`.
    TraverseUserGroups { user: String },
    /// IAM: filter to one user, drill into their attached managed policies.
    TraverseUserPolicies { user: String },
    /// IAM: filter to one user, drill into their access keys.
    TraverseUserAccessKeys { user: String },
    /// IAM: filter to one role, drill into its attached managed policies.
    TraverseRolePolicies { role: String },
    /// IAM: filter to one role, drill into the instance profiles wrapping it.
    TraverseRoleProfiles { role: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(region) = &cli.region {
        // SAFETY: single-threaded, before any other env reads.
        unsafe { std::env::set_var("AWS_REGION", region) };
    }
    let aws = AwsAccount::from_default().context(
        "Set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/AWS_REGION, or configure ~/.aws/credentials [default]",
    )?;

    match cli.command {
        Command::LogGroups { prefix } => {
            let mut t = groups_table(aws);
            if let Some(p) = prefix {
                t.add_condition(eq("logGroupNamePrefix", p));
            }
            print_table(&t).await?;
        }

        Command::LogStreams { log_group_name } => {
            let mut t = streams_table(aws);
            t.add_condition(eq("logGroupName", log_group_name));
            print_table(&t).await?;
        }

        Command::LogEvents { log_group_name } => {
            let mut t = events_table(aws);
            t.add_condition(eq("logGroupName", log_group_name));
            print_table(&t).await?;
        }

        Command::TraverseLogEvents => {
            let mut log_groups = groups_table(aws);
            log_groups.add_condition(eq("logGroupNamePrefix", TRAVERSE_GROUP));
            print_table(&log_groups).await?;

            println!("\n→ events via with_many(\"events\"):\n");

            let events_any = log_groups.get_ref("events")?;
            let records = events_any.list_values().await?;
            render_records(&records, Some("eventId"));
        }

        Command::TraverseLogStreams => {
            let mut log_groups = groups_table(aws);
            log_groups.add_condition(eq("logGroupNamePrefix", TRAVERSE_GROUP));
            print_table(&log_groups).await?;

            println!("\n→ streams via with_many(\"streams\"):\n");

            let streams_any = log_groups.get_ref("streams")?;
            let records = streams_any.list_values().await?;
            render_records(&records, Some("logStreamName"));
        }

        Command::ListClusters => {
            let t = clusters_table(aws);
            print_table(&t).await?;
        }

        Command::ListServices { cluster } => {
            let mut t = services_table(aws);
            t.add_condition(eq("cluster", cluster));
            print_table(&t).await?;
        }

        Command::ListTasks {
            cluster,
            service,
            family,
            status,
        } => {
            let mut t = tasks_table(aws);
            t.add_condition(eq("cluster", cluster));
            if let Some(s) = service {
                t.add_condition(eq("serviceName", s));
            }
            if let Some(f) = family {
                t.add_condition(eq("family", f));
            }
            if let Some(s) = status {
                t.add_condition(eq("desiredStatus", s));
            }
            print_table(&t).await?;
        }

        Command::ListTaskDefs { family_prefix } => {
            let mut t = task_definitions_table(aws);
            if let Some(p) = family_prefix {
                t.add_condition(eq("familyPrefix", p));
            }
            print_table(&t).await?;
        }

        Command::ListUsers { path_prefix } => {
            let mut t = users_table(aws);
            if let Some(p) = path_prefix {
                t.add_condition(eq("PathPrefix", p));
            }
            print_table(&t).await?;
        }

        Command::ListGroups { path_prefix } => {
            let mut t = iam_groups_table(aws);
            if let Some(p) = path_prefix {
                t.add_condition(eq("PathPrefix", p));
            }
            print_table(&t).await?;
        }

        Command::ListRoles { path_prefix } => {
            let mut t = roles_table(aws);
            if let Some(p) = path_prefix {
                t.add_condition(eq("PathPrefix", p));
            }
            print_table(&t).await?;
        }

        Command::ListPolicies {
            scope,
            only_attached,
            path_prefix,
        } => {
            let mut t = policies_table(aws);
            if let Some(s) = scope {
                t.add_condition(eq("Scope", s));
            }
            if let Some(o) = only_attached {
                t.add_condition(eq("OnlyAttached", o));
            }
            if let Some(p) = path_prefix {
                t.add_condition(eq("PathPrefix", p));
            }
            print_table(&t).await?;
        }

        Command::ListAccessKeys { user } => {
            let mut t = access_keys_table(aws);
            if let Some(u) = user {
                t.add_condition(eq("UserName", u));
            }
            print_table(&t).await?;
        }

        Command::ListInstanceProfiles { path_prefix } => {
            let mut t = instance_profiles_table(aws);
            if let Some(p) = path_prefix {
                t.add_condition(eq("PathPrefix", p));
            }
            print_table(&t).await?;
        }

        // IAM ListUsers / ListRoles ignore unknown filters and return
        // the whole account, so the parent table can't be narrowed
        // API-side. The entity-method helpers (`User::ref_*`,
        // `Role::ref_*`) are the right tool here — they take the
        // already-loaded entity and pre-filter the child table.
        Command::TraverseUserGroups { user } => {
            let target = find_user(aws.clone(), &user).await?;
            print_table(&target.ref_groups(aws)).await?;
        }

        Command::TraverseUserPolicies { user } => {
            let target = find_user(aws.clone(), &user).await?;
            print_table(&target.ref_attached_policies(aws)).await?;
        }

        Command::TraverseUserAccessKeys { user } => {
            let target = find_user(aws.clone(), &user).await?;
            print_table(&target.ref_access_keys(aws)).await?;
        }

        Command::TraverseRolePolicies { role } => {
            let target = find_role(aws.clone(), &role).await?;
            print_table(&target.ref_attached_policies(aws)).await?;
        }

        Command::TraverseRoleProfiles { role } => {
            let target = find_role(aws.clone(), &role).await?;
            print_table(&target.ref_instance_profiles(aws)).await?;
        }
    }

    Ok(())
}

async fn find_user(aws: AwsAccount, name: &str) -> Result<User> {
    users_table(aws)
        .get(&name.to_string())
        .await?
        .with_context(|| format!("IAM user {name:?} not found in this account"))
}

async fn find_role(aws: AwsAccount, name: &str) -> Result<Role> {
    roles_table(aws)
        .get(&name.to_string())
        .await?
        .with_context(|| format!("IAM role {name:?} not found in this account"))
}
