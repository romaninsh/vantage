//! Vista-driven CLI for browsing bakery data backed by CSV files.
//!
//! Usage: db-vista <entity> [command ...]
//!
//! Entities: bakery, client, product, order
//!
//! Commands:
//!   list              List all records as a table
//!   get               Show first record in detail
//!   count             Count records
//!   add <id> <json>   Insert a record (CSV is read-only — errors out)
//!   delete <id>       Delete a record by ID (CSV is read-only — errors out)

use bakery_model3::*;
use clap::{Arg, Command};
use vantage_cli_util::render_records;
use vantage_csv::Csv;
use vantage_dataset::prelude::*;
use vantage_vista::Vista;

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
    let app = Command::new("db-vista")
        .about("Vista-driven CLI for the CSV bakery dataset")
        .arg(
            Arg::new("entity")
                .help("Entity name (bakery, client, product, order)")
                .required(false),
        )
        .arg(
            Arg::new("commands")
                .help("Commands: list, get, count, add <id> <json>, delete <id>")
                .num_args(0..)
                .trailing_var_arg(true),
        );

    let matches = app.get_matches();

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

    let vista = match build_vista(&entity_name)? {
        Some(v) => v,
        None => return Ok(()),
    };

    handle_commands(vista, commands).await
}

fn build_vista(entity_name: &str) -> vantage_core::Result<Option<Vista>> {
    let csv = Csv::new("bakery_model3/data");
    let factory = csv.vista_factory();
    let vista = match entity_name {
        "bakery" => factory.from_table(Bakery::csv_table(csv))?,
        "client" => factory.from_table(Client::csv_table(csv))?,
        "product" => factory.from_table(Product::csv_table(csv))?,
        "order" => factory.from_table(Order::csv_table(csv))?,
        _ => {
            println!("Unknown entity: {}", entity_name);
            return Ok(None);
        }
    };
    Ok(Some(vista))
}

fn print_usage() {
    println!();
    println!("Usage: db-vista <entity> <command>");
    println!();
    println!("Commands:");
    println!("  list              List all records");
    println!("  get               Get first record (detailed)");
    println!("  count             Count records");
    println!("  add <id> <json>   Insert a record");
    println!("  delete <id>       Delete a record by ID");
    println!();
    println!("Examples:");
    println!("  db-vista bakery list");
    println!("  db-vista product count");
    println!("  db-vista client get");
}

async fn handle_commands(vista: Vista, commands: Vec<String>) -> vantage_core::Result<()> {
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
                let records = vista.list_values().await?;
                render_records(&records, None);
            }
            "get" => match vista.get_some_value().await? {
                Some((id, record)) => {
                    println!("id: {}", id);
                    for (k, v) in record.iter() {
                        println!("  {}: {}", k, serde_json::to_string(v).unwrap_or_default());
                    }
                }
                None => println!("No records found"),
            },
            "count" => {
                let count = vista.get_count().await?;
                println!("{} records", count);
            }
            "add" => {
                if i + 1 >= commands.len() {
                    println!("Usage: add <id> <json>");
                    break;
                }
                let id = commands[i].clone();
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

                let cbor_val = ciborium::Value::serialized(&json_val).map_err(|e| {
                    vantage_core::error!("Invalid JSON for CBOR", details = e.to_string())
                })?;
                let record = vantage_types::Record::from(cbor_val);
                vista.insert_value(&id, &record).await?;
                println!("Inserted: {}", id);
            }
            "delete" => {
                if i >= commands.len() {
                    println!("Usage: delete <id>");
                    break;
                }
                let id = commands[i].clone();
                i += 1;

                vista.delete(&id).await?;
                println!("Deleted: {}", id);
            }
            other => {
                println!("Unknown command: {}", other);
                println!("Available: list, get, count, add <id> <json>, delete <id>");
            }
        }
    }
    Ok(())
}
