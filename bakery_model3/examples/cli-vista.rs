//! Vista-driven multi-source CLI for browsing and managing bakery data.
//!
//! Usage: db-vista [--debug] <source> <entity> [command ...]
//!
//! Sources: csv, sqlite, postgres, mongo
//! Entities: bakery, client, product, order
//!
//! Commands:
//!   list              List all records as a table
//!   get               Show first record in detail
//!   count             Count records
//!   add <id> <json>   Insert a record
//!   delete <id>       Delete a record by ID

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
        .about("Vista-driven CLI for the bakery dataset")
        .arg(
            Arg::new("debug")
                .long("debug")
                .help("Enable debug mode (show queries)")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("source")
                .help("Data source: csv, sqlite, postgres, mongo")
                .required(true),
        )
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
    let _debug = matches.get_flag("debug");
    let source = matches.get_one::<String>("source").unwrap();

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

    let vista = match build_vista(source, &entity_name).await? {
        Some(v) => v,
        None => return Ok(()),
    };

    handle_commands(vista, commands).await
}

async fn build_vista(source: &str, entity_name: &str) -> vantage_core::Result<Option<Vista>> {
    match source {
        "csv" => {
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
        "sqlite" => {
            let db = SqliteDB::connect("sqlite:target/bakery.sqlite")
                .await
                .map_err(|e| {
                    vantage_core::error!("Failed to connect to SQLite", details = e.to_string())
                })?;
            let factory = db.vista_factory();
            let vista = match entity_name {
                "bakery" => factory.from_table(Bakery::sqlite_table(db))?,
                "client" => factory.from_table(Client::sqlite_table(db))?,
                "product" => factory.from_table(Product::sqlite_table(db))?,
                "order" => factory.from_table(Order::sqlite_table(db))?,
                _ => {
                    println!("Unknown entity: {}", entity_name);
                    return Ok(None);
                }
            };
            Ok(Some(vista))
        }
        "postgres" => {
            let url = std::env::var("POSTGRES_URL").unwrap_or_else(|_| {
                "postgres://vantage:vantage@localhost:5433/vantage".to_string()
            });
            let db = PostgresDB::connect(&url).await.map_err(|e| {
                vantage_core::error!("Failed to connect to PostgreSQL", details = e.to_string())
            })?;
            let factory = db.vista_factory();
            let vista = match entity_name {
                "bakery" => factory.from_table(Bakery::postgres_table(db))?,
                "client" => factory.from_table(Client::postgres_table(db))?,
                "product" => factory.from_table(Product::postgres_table(db))?,
                "order" => factory.from_table(Order::postgres_table(db))?,
                _ => {
                    println!("Unknown entity: {}", entity_name);
                    return Ok(None);
                }
            };
            Ok(Some(vista))
        }
        "mongo" => {
            let url = std::env::var("MONGODB_URL")
                .unwrap_or_else(|_| "mongodb://localhost:27017".to_string());
            let db_name = std::env::var("MONGODB_DB").unwrap_or_else(|_| "vantage".to_string());
            let db = MongoDB::connect(&url, &db_name).await.map_err(|e| {
                vantage_core::error!("Failed to connect to MongoDB", details = e.to_string())
            })?;
            let factory = db.vista_factory();
            let vista = match entity_name {
                "bakery" => factory.from_table(Bakery::mongo_table(db))?,
                "client" => factory.from_table(Client::mongo_table(db))?,
                "product" => factory.from_table(Product::mongo_table(db))?,
                "order" => factory.from_table(Order::mongo_table(db))?,
                _ => {
                    println!("Unknown entity: {}", entity_name);
                    return Ok(None);
                }
            };
            Ok(Some(vista))
        }
        _ => {
            println!(
                "Unknown source: {}. Use: csv, sqlite, postgres, mongo",
                source
            );
            Ok(None)
        }
    }
}

fn print_usage() {
    println!();
    println!("Usage: db-vista [--debug] <source> <entity> <command>");
    println!();
    println!("Sources: csv, sqlite, postgres, mongo");
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
    println!("  db-vista csv bakery list");
    println!("  db-vista sqlite product list");
    println!("  db-vista mongo bakery count");
    println!(r#"  db-vista postgres bakery add myid '{{"name":"Test","profit_margin":10}}'"#);
    println!("  db-vista postgres bakery delete myid");
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
