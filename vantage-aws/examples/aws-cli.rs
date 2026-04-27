//! `vantage-aws-cli` — proof-of-concept CLI driving the CloudWatch
//! and ECS models through `vantage-aws`. Renders results via
//! `vantage_cli_util::print_table` so the output exercises the same
//! `Table` / `TableSource` machinery the rest of the framework uses.
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
use vantage_aws::models::{log_events_table, log_groups_table, log_streams_table};
use vantage_aws::{AwsAccount, eq};
use vantage_cli_util::{print_table, render_records};
use vantage_dataset::prelude::ReadableValueSet;

const TRAVERSE_GROUP: &str = "/ecs/ba-nginx";

#[derive(Parser)]
#[command(name = "vantage-aws-cli", about = "vantage-aws CloudWatch + ECS demo")]
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
    ListGroups {
        #[arg(long)]
        prefix: Option<String>,
    },
    /// CloudWatch: list log streams in a group.
    ListStreams { log_group_name: String },
    /// CloudWatch: list log events in a group.
    ListEvents { log_group_name: String },
    /// CloudWatch: filter to one group, drill into its events via the `events` relation.
    Traverse,
    /// CloudWatch: filter to one group, drill into its streams via the `streams` relation.
    TraverseStreams,

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
        Command::ListGroups { prefix } => {
            let mut t = log_groups_table(aws);
            if let Some(p) = prefix {
                t.add_condition(eq("logGroupNamePrefix", p));
            }
            print_table(&t).await?;
        }

        Command::ListStreams { log_group_name } => {
            let mut t = log_streams_table(aws);
            t.add_condition(eq("logGroupName", log_group_name));
            print_table(&t).await?;
        }

        Command::ListEvents { log_group_name } => {
            let mut t = log_events_table(aws);
            t.add_condition(eq("logGroupName", log_group_name));
            print_table(&t).await?;
        }

        Command::Traverse => {
            let mut groups_table = log_groups_table(aws);
            groups_table.add_condition(eq("logGroupNamePrefix", TRAVERSE_GROUP));
            print_table(&groups_table).await?;

            println!("\n→ events via with_many(\"events\"):\n");

            let events_any = groups_table.get_ref("events")?;
            let records = events_any.list_values().await?;
            render_records(&records, Some("eventId"));
        }

        Command::TraverseStreams => {
            let mut groups_table = log_groups_table(aws);
            groups_table.add_condition(eq("logGroupNamePrefix", TRAVERSE_GROUP));
            print_table(&groups_table).await?;

            println!("\n→ streams via with_many(\"streams\"):\n");

            let streams_any = groups_table.get_ref("streams")?;
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
    }

    Ok(())
}
