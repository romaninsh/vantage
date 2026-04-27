//! `vantage-aws-cli` — proof-of-concept CLI driving the CloudWatch
//! models through `vantage-aws`. Renders results via
//! `vantage_cli_util::print_table` so the output exercises the same
//! `Table` / `TableSource` machinery the rest of the framework uses.
//!
//! Reads creds from the standard env vars (`AWS_ACCESS_KEY_ID`,
//! `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, `AWS_REGION`).
//!
//! Subcommands:
//!   list-groups [--prefix <p>]   → DescribeLogGroups
//!   list-events <group>           → FilterLogEvents
//!   traverse                      → filter to one group, drill into its events

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use vantage_aws::models::{log_events_table, log_groups_table};
use vantage_aws::{AwsAccount, eq};
use vantage_cli_util::{print_table, render_records};
use vantage_dataset::prelude::ReadableValueSet;

const TRAVERSE_GROUP: &str = "/ecs/ba-nginx";

#[derive(Parser)]
#[command(name = "vantage-aws-cli", about = "vantage-aws CloudWatch demo")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List CloudWatch log groups (optionally filtered by prefix).
    ListGroups {
        #[arg(long)]
        prefix: Option<String>,
    },
    /// List log events for a specific log group.
    ListEvents { log_group_name: String },
    /// Demo the relation: filter to a specific log group, then list its events.
    Traverse,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let aws = AwsAccount::from_env().context(
        "Set AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION (and optionally AWS_SESSION_TOKEN)",
    )?;

    match cli.command {
        Command::ListGroups { prefix } => {
            let mut t = log_groups_table(aws);
            if let Some(p) = prefix {
                t.add_condition(eq("logGroupNamePrefix", p));
            }
            print_table(&t).await?;
        }

        Command::ListEvents { log_group_name } => {
            let mut t = log_events_table(aws);
            t.add_condition(eq("logGroupName", log_group_name));
            print_table(&t).await?;
        }

        Command::Traverse => {
            // Narrow the source table to the target group (DescribeLogGroups
            // takes `logGroupNamePrefix`, and the full name is a prefix of
            // itself). The `with_foreign("events", ...)` closure registered
            // in log_groups_table picks this up and pre-conditions the
            // events table with the same group.
            let mut groups_table = log_groups_table(aws);
            groups_table.add_condition(eq("logGroupNamePrefix", TRAVERSE_GROUP));
            print_table(&groups_table).await?;

            println!("\n→ events via with_foreign(\"events\"):\n");

            let events_any = groups_table.get_ref("events")?;
            let records = events_any.list_values().await?;
            render_records(&records, Some("eventId"));
        }
    }

    Ok(())
}
