//! Vista-driven CLI for the YAML-defined bakery_model4 schema.
//!
//! Usage: db4 <entity> [command ...]
//!
//! Entities: bakery, client, product, order
//!
//! Commands:
//!   list              List all records as a table
//!   get               Show first record in detail
//!   count             Count records
//!   add <id> <json>   Insert a record
//!   delete <id>       Delete a record by ID
//!   caps              Show what this source supports

use bakery_model4::{connect_sqlite, entity_names, vista};
use clap::{Arg, Command};
use vantage_cli_util::render_records;
use vantage_dataset::prelude::*;
use vantage_vista::Vista;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> vantage_core::Result<()> {
    let app = Command::new("db4")
        .about("Vista-driven CLI for the YAML-defined bakery_model4 (SQLite)")
        .arg(
            Arg::new("entity")
                .help("Entity name (bakery, client, product, order)")
                .required(false),
        )
        .arg(
            Arg::new("commands")
                .help("Commands: list, get, count, add <id> <json>, delete <id>, caps")
                .num_args(0..)
                .trailing_var_arg(true),
        );

    let matches = app.get_matches();

    let entity_name = match matches.get_one::<String>("entity") {
        Some(name) => name.clone(),
        None => {
            println!("Available entities: {}", entity_names().join(", "));
            print_usage();
            return Ok(());
        }
    };

    let commands: Vec<String> = matches
        .get_many::<String>("commands")
        .unwrap_or_default()
        .cloned()
        .collect();

    let db = connect_sqlite().await?;
    let vista = vista(db, &entity_name)?;

    handle_commands(vista, commands).await
}

fn print_usage() {
    println!();
    println!("Usage: db4 <entity> <command>");
    println!();
    println!("Commands:");
    println!("  list              List all records");
    println!("  get               Get first record (detailed)");
    println!("  count             Count records");
    println!("  add <id> <json>   Insert a record");
    println!("  delete <id>       Delete a record by ID");
    println!("  caps              Show what this source supports");
    println!();
    println!("Examples:");
    println!("  db4 bakery list");
    println!("  db4 product count");
    println!(r#"  db4 bakery add my_bakery '{{"name":"Test","profit_margin":10}}'"#);
}

fn print_capabilities(vista: &Vista) {
    let c = vista.capabilities();
    println!(
        "Capabilities for '{}' (driver: {}):",
        vista.name(),
        vista.driver()
    );
    println!("  list/get      yes (always)");
    println!("  count         {}", yes_no(c.can_count));
    println!("  add (insert)  {}", yes_no(c.can_insert));
    println!("  update        {}", yes_no(c.can_update));
    println!("  delete        {}", yes_no(c.can_delete));
    println!("  subscribe     {}", yes_no(c.can_subscribe));
    println!("  invalidate    {}", yes_no(c.can_invalidate));
    println!("  pagination    {:?}", c.paginate_kind);
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
}

async fn handle_commands(vista: Vista, commands: Vec<String>) -> vantage_core::Result<()> {
    if commands.is_empty() {
        println!("No command. Try: list, get, count, add, delete, caps");
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
                if !vista.capabilities().can_count {
                    println!(
                        "'{}' does not support counting — skipping (try 'list' instead).",
                        vista.name()
                    );
                    continue;
                }
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

                if !vista.capabilities().can_insert {
                    println!(
                        "'{}' is read-only — insert is not supported by this source.",
                        vista.name()
                    );
                    continue;
                }

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

                if !vista.capabilities().can_delete {
                    println!(
                        "'{}' is read-only — delete is not supported by this source.",
                        vista.name()
                    );
                    continue;
                }

                vista.delete(&id).await?;
                println!("Deleted: {}", id);
            }
            "caps" => {
                print_capabilities(&vista);
            }
            other => {
                println!("Unknown command: {}", other);
                println!("Available: list, get, count, add <id> <json>, delete <id>, caps");
            }
        }
    }
    Ok(())
}
