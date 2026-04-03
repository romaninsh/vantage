use bakery_model3::*;
use clap::{Arg, Command};
use vantage_cli_util::print_table;
use vantage_core::util::error::Context;
use vantage_csv::Csv;
use vantage_dataset::prelude::*;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, IntoRecord, Record, TerminalRender};

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
        .about("Database management utility for Bakery")
        .arg(
            Arg::new("debug")
                .long("debug")
                .help("Enable debug mode (show queries)")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("source")
                .help("Data source: csv, surreal")
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
    let debug = matches.get_flag("debug");
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

    match source.as_str() {
        "csv" => {
            let csv = Csv::new("bakery_model3/data");
            match entity_name.as_str() {
                "bakery" => handle_read(Bakery::csv_table(csv), commands).await?,
                "client" => handle_read(Client::csv_table(csv), commands).await?,
                "product" => handle_read(Product::csv_table(csv), commands).await?,
                "order" => handle_read(Order::csv_table(csv), commands).await?,
                _ => println!("Unknown entity: {}", entity_name),
            }
        }
        "surreal" => {
            connect_surrealdb_with_debug(debug)
                .await
                .context("Failed to connect to SurrealDB")?;
            let db = surrealdb();
            match entity_name.as_str() {
                "bakery" => handle_surreal(Bakery::surreal_table(db), commands).await?,
                "client" => handle_surreal(Client::surreal_table(db), commands).await?,
                "product" => handle_surreal(Product::surreal_table(db), commands).await?,
                "order" => handle_surreal(Order::surreal_table(db), commands).await?,
                _ => println!("Unknown entity: {}", entity_name),
            }
        }
        _ => println!("Unknown source: {}. Use: csv, surreal", source),
    }

    Ok(())
}

fn print_usage() {
    println!();
    println!("Usage: db [--debug] <source> <entity> <command>");
    println!();
    println!("Sources: csv, surreal");
    println!();
    println!("Commands:");
    println!("  list              List all records");
    println!("  get               Get first record (detailed)");
    println!("  count             Count records");
    println!("  add <id> <json>   Insert a record (surreal)");
    println!("  delete <id>       Delete a record (surreal)");
    println!();
    println!("Examples:");
    println!("  db csv bakery list");
    println!("  db surreal product list");
    println!("  db surreal bakery count");
    println!(r#"  db surreal bakery add myid '{{"name":"Test","profit_margin":10}}'"#);
    println!("  db surreal bakery delete myid");
}

// ── Generic read commands (any source) ──────────────────────────────────

async fn read_command<T, E>(table: &Table<T, E>, command: &str) -> vantage_core::Result<bool>
where
    T: TableSource,
    T::Value: TerminalRender,
    T::Id: std::fmt::Display,
    E: Entity<T::Value> + std::fmt::Debug,
{
    match command {
        "list" => {
            print_table(table).await?;
            Ok(true)
        }
        "get" => {
            match table.get_some().await? {
                Some((id, entity)) => {
                    println!("id: {}", id);
                    println!("{:#?}", entity);
                }
                None => println!("No records found"),
            }
            Ok(true)
        }
        "count" => {
            let count = table.data_source().get_count(table).await?;
            println!("{} records", count);
            Ok(true)
        }
        _ => Ok(false),
    }
}

// ── CSV (read-only) ─────────────────────────────────────────────────────

async fn handle_read<T, E>(table: Table<T, E>, commands: Vec<String>) -> vantage_core::Result<()>
where
    T: TableSource,
    T::Value: TerminalRender,
    T::Id: std::fmt::Display,
    E: Entity<T::Value> + std::fmt::Debug,
{
    if commands.is_empty() {
        println!("No command. Try: list, get, count");
        return Ok(());
    }
    for cmd in &commands {
        if !read_command(&table, cmd).await? {
            println!("Unknown command: {}", cmd);
            println!("Available: list, get, count");
        }
    }
    Ok(())
}

// ── SurrealDB (read + write) ───────────────────────────────────────────

async fn handle_surreal<E>(
    table: Table<SurrealDB, E>,
    commands: Vec<String>,
) -> vantage_core::Result<()>
where
    E: Entity<AnySurrealType> + std::fmt::Debug,
{
    if commands.is_empty() {
        println!("No command. Try: list, get, count, add, delete");
        return Ok(());
    }

    let mut i = 0;
    while i < commands.len() {
        let cmd = &commands[i];
        i += 1;

        if read_command(&table, cmd).await? {
            continue;
        }

        match cmd.as_str() {
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

                // serde_json::Value -> Record<serde_json::Value> -> Record<AnySurrealType>
                let json_record = Record::<serde_json::Value>::from(json_val);
                let record: Record<AnySurrealType> = json_record.into_record();

                let thing = Thing::new(table.table_name(), id_str.as_str());
                let returned = table.insert_value(&thing, &record).await?;

                println!("Inserted (id={}):", thing);
                for (k, v) in returned.iter() {
                    println!("  {}: {}", k, v.render());
                }
            }
            "delete" => {
                if i >= commands.len() {
                    println!("Usage: delete <id>");
                    break;
                }
                let id_str = &commands[i];
                i += 1;

                let thing = Thing::new(table.table_name(), id_str.as_str());
                WritableValueSet::delete(&table, &thing).await?;
                println!("Deleted: {}", thing);
            }
            other => {
                println!("Unknown command: {}", other);
                println!("Available: list, get, count, add <id> <json>, delete <id>");
            }
        }
    }
    Ok(())
}
