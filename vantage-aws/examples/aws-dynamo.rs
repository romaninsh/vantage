//! `aws-dynamo` — list every DynamoDB table in the configured account/region
//! and dump each table's contents via Scan.
//!
//! Mirrors what you'd do with the AWS CLI:
//!
//! ```sh
//! aws dynamodb list-tables --region eu-west-2
//! aws dynamodb scan --table-name <name> --region eu-west-2
//! ```
//!
//! Credentials: reads `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` /
//! `AWS_SESSION_TOKEN` / `AWS_REGION` from the env if set; otherwise
//! resolves the profile named by `AWS_PROFILE` (default: `default`)
//! from `~/.aws/credentials`. SSO and assume-role profiles are
//! resolved by shelling out to `aws configure export-credentials`,
//! so `AWS_PROFILE=<sso_profile> cargo run --example aws-dynamo`
//! just works as long as `aws sso login` is current.
//!
//! v0 caveat: `DynamoDB::list_table_values` keys items by a single id
//! field (default `"id"`). If the actual hash key is something else
//! (e.g. `PK` for single-table designs), pass `--id-field PK`.
//! Tables with composite (PK+SK) keys still scan fine; only the id
//! used as the IndexMap key changes.

use anyhow::{Context, Result};
use clap::Parser;
use vantage_aws::AwsAccount;
use vantage_aws::dynamodb::DynamoDB;
use vantage_aws::models::dynamodb::tables_table;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_table::traits::table_like::TableLike;
use vantage_types::EmptyEntity;

#[derive(Parser)]
#[command(
    name = "aws-dynamo",
    about = "List DynamoDB tables and dump their contents"
)]
struct Cli {
    /// Override the AWS region (sets AWS_REGION before credential load).
    #[arg(long)]
    region: Option<String>,

    /// Field to treat as the row id when scanning. v0 vantage-aws
    /// doesn't auto-detect the hash key, so single-table designs
    /// using `PK` need this flag set.
    #[arg(long, default_value = "id")]
    id_field: String,

    /// Limit how many items to print per table (full scan still runs).
    #[arg(long, default_value_t = 20)]
    sample: usize,

    /// Only scan tables whose name contains this substring.
    #[arg(long)]
    filter: Option<String>,

    /// Skip the per-table contents dump — list names + counts only.
    #[arg(long)]
    no_scan: bool,
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

    let names: Vec<String> = tables_table(aws.clone())
        .list_values()
        .await
        .context("ListTables failed")?
        .into_keys()
        .collect();

    let region_label = std::env::var("AWS_REGION").unwrap_or_else(|_| "<unset>".into());
    println!("region: {}", region_label);
    println!("tables ({}):", names.len());

    let db = DynamoDB::new(aws);
    for name in &names {
        let count_table: Table<DynamoDB, EmptyEntity> = Table::new(name.as_str(), db.clone());
        let count = match count_table.get_count().await {
            Ok(n) => format!("{} items", n),
            Err(e) => format!("count failed: {}", e),
        };
        println!("  - {}  ({})", name, count);
    }
    println!();

    if cli.no_scan {
        return Ok(());
    }

    for name in &names {
        if let Some(needle) = &cli.filter
            && !name.contains(needle)
        {
            continue;
        }
        scan_table(&db, name, &cli.id_field, cli.sample).await;
        println!();
    }
    Ok(())
}

async fn scan_table(db: &DynamoDB, name: &str, id_field: &str, sample: usize) {
    let table: Table<DynamoDB, EmptyEntity> = Table::new(name, db.clone()).with_id_column(id_field);

    println!("=== {} (id={}) ===", name, id_field);
    match table.list_values().await {
        Ok(items) if items.is_empty() => println!("(empty)"),
        Ok(items) => {
            let total = items.len();
            for (i, (id, rec)) in items.iter().enumerate() {
                if i >= sample {
                    println!("... ({} more)", total - sample);
                    break;
                }
                println!("- {}={}", id_field, id);
                for (k, v) in rec.iter() {
                    if k == id_field {
                        continue;
                    }
                    let json = serde_json::Value::from(v.clone());
                    println!("    {}: {}", k, json);
                }
            }
        }
        Err(e) => println!("scan failed: {}", e),
    }
}
