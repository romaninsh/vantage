//! Multi-backend CLI for browsing and managing bakery data.
//!
//! Usage: db [--debug] [--backend surreal|dynamodb] <entity> [command ...]
//!
//! Entities: bakery, client, product, order
//!
//! Commands:
//!   list              List all records as a table
//!   get               Show first record in detail
//!   count             Count records
//!   add <id> <json>   Insert a record
//!   delete <id>       Delete a record by ID
//!   ref <name>        Traverse a relationship and list the target table

use bakery_model3::*;
use clap::{Arg, Command};
use vantage_cli_util::render_records;
use vantage_core::util::error::Context;
use vantage_dataset::prelude::*;
use vantage_table::any::AnyTable;
use vantage_table::traits::table_like::TableLike;

const DYNAMO_REGION: &str = "eu-west-2";

fn model_names() -> Vec<&'static str> {
    vec!["bakery", "client", "product", "order"]
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> vantage_core::Result<()> {
    let app = Command::new("db")
        .about("Multi-backend management utility for Bakery")
        .arg(
            Arg::new("debug")
                .long("debug")
                .help("Enable debug mode (show queries)")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("backend")
                .long("backend")
                .help("Backend to use: surreal (default) or dynamodb")
                .value_parser(["surreal", "dynamodb"])
                .default_value("surreal")
                .global(true),
        )
        .arg(
            Arg::new("entity")
                .help("Entity name (bakery, client, product, order)")
                .required(false),
        )
        .arg(
            Arg::new("commands")
                .help("Commands: list, get, count, add <id> <json>, delete <id>, ref <name>")
                .num_args(0..)
                .trailing_var_arg(true),
        );

    let matches = app.get_matches();
    let debug = matches.get_flag("debug");
    let backend = matches.get_one::<String>("backend").cloned().unwrap_or_else(|| "surreal".into());

    let entity_name = match matches.get_one::<String>("entity") {
        Some(name) => name.clone(),
        None => {
            println!("Available entities: {}", model_names().join(", "));
            print_usage();
            return Ok(());
        }
    };

    let commands: Vec<String> = matches
        .get_many::<String>("commands")
        .unwrap_or_default()
        .cloned()
        .collect();

    let table = match build_table(&entity_name, &backend, debug).await? {
        Some(t) => t,
        None => return Ok(()),
    };

    handle_commands(table, commands, &backend).await
}

async fn build_table(
    entity_name: &str,
    backend: &str,
    debug: bool,
) -> vantage_core::Result<Option<AnyTable>> {
    match backend {
        "surreal" => {
            connect_surrealdb_with_debug(debug)
                .await
                .context("Failed to connect to SurrealDB")?;
            let db = surrealdb();
            let table = match entity_name {
                "bakery" => AnyTable::from_table(Bakery::surreal_table(db)),
                "client" => AnyTable::from_table(Client::surreal_table(db)),
                "product" => AnyTable::from_table(Product::surreal_table(db)),
                "order" => AnyTable::from_table(Order::surreal_table(db)),
                _ => {
                    println!("Unknown entity: {}", entity_name);
                    return Ok(None);
                }
            };
            Ok(Some(table))
        }
        "dynamodb" => {
            let aws = AwsAccount::from_default()
                .context("Failed to load AWS credentials (env or ~/.aws/credentials)")?
                .with_region(DYNAMO_REGION);
            let db = DynamoDB::new(aws);
            let table = match entity_name {
                "bakery" => AnyTable::from_table(Bakery::dynamo_table(db)),
                "client" => AnyTable::from_table(Client::dynamo_table(db)),
                "product" => AnyTable::from_table(Product::dynamo_table(db)),
                "order" => AnyTable::from_table(Order::dynamo_table(db)),
                _ => {
                    println!("Unknown entity: {}", entity_name);
                    return Ok(None);
                }
            };
            Ok(Some(table))
        }
        other => {
            println!("Unknown backend: {}", other);
            Ok(None)
        }
    }
}

fn print_usage() {
    println!();
    println!("Usage: db [--debug] <entity> <command>");
    println!();
    println!("Commands:");
    println!("  list              List all records");
    println!("  get               Get first record (detailed)");
    println!("  count             Count records");
    println!("  add <id> <json>   Insert a record");
    println!("  delete <id>       Delete a record by ID");
    println!();
    println!("Examples:");
    println!("  db bakery list");
    println!("  db product list");
    println!("  db bakery count");
    println!(r#"  db bakery add myid '{{"name":"Test","profit_margin":10}}'"#);
    println!("  db bakery delete myid");
}

async fn handle_commands(
    table: AnyTable,
    commands: Vec<String>,
    backend: &str,
) -> vantage_core::Result<()> {
    if commands.is_empty() {
        println!("No command. Try: list, get, count, add, delete");
        return Ok(());
    }

    let mut i = 0;
    while i < commands.len() {
        let cmd = &commands[i];
        i += 1;

        match cmd.as_str() {
            "list" => {
                let records = table.list_values().await?;
                render_records(&records, None);
            }
            "get" => match table.get_some_value().await? {
                Some((id, record)) => {
                    println!("id: {}", id);
                    for (k, v) in record.iter() {
                        println!("  {}: {}", k, serde_json::to_string(v).unwrap_or_default());
                    }
                }
                None => println!("No records found"),
            },
            "count" => {
                let count = table.get_count().await?;
                println!("{} records", count);
            }
            "add" => {
                if i + 1 >= commands.len() {
                    println!("Usage: add <id> <json>");
                    break;
                }
                let id_str = &commands[i];
                i += 1;
                let json_str = &commands[i];
                i += 1;

                let json_val: serde_json::Value =
                    serde_json::from_str(json_str).map_err(|e: serde_json::Error| {
                        vantage_core::error!("Invalid JSON", details = e.to_string())
                    })?;

                if !json_val.is_object() {
                    println!("Error: JSON must be an object, e.g. '{{\"name\":\"value\"}}'");
                    break;
                }

                let id = qualify_id(table.table_name(), id_str, backend);
                let cbor_val = ciborium::Value::serialized(&json_val).map_err(|e| {
                    vantage_core::error!("Invalid JSON for CBOR", details = e.to_string())
                })?;
                let record = vantage_types::Record::from(cbor_val);
                table.insert_value(&id, &record).await?;
                println!("Inserted: {}", id);
            }
            "delete" => {
                if i >= commands.len() {
                    println!("Usage: delete <id>");
                    break;
                }
                let id_str = &commands[i];
                i += 1;

                let id = qualify_id(table.table_name(), id_str, backend);
                table.delete(&id).await?;
                println!("Deleted: {}", id);
            }
            "ref" => {
                if i >= commands.len() {
                    println!("Usage: ref <relation-name>");
                    break;
                }
                let ref_name = commands[i].clone();
                i += 1;

                let related = table.get_ref(&ref_name)?;
                let records = related.list_values().await?;
                println!(
                    "[{}] traversed via {} → {} record(s)",
                    related.table_name(),
                    ref_name,
                    records.len()
                );
                render_records(&records, None);
            }
            other => {
                println!("Unknown command: {}", other);
                println!(
                    "Available: list, get, count, add <id> <json>, delete <id>, ref <name>"
                );
            }
        }
    }
    Ok(())
}

/// Qualify a bare id. SurrealDB wants the `table:id` form; everything
/// else (DynamoDB, etc.) takes the id verbatim.
fn qualify_id(table_name: &str, id: &str, backend: &str) -> String {
    if backend == "surreal" && !id.contains(':') {
        format!("{}:{}", table_name, id)
    } else {
        id.to_string()
    }
}
