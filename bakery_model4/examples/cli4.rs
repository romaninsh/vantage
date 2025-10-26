use bakery_model4::{
    connect_surrealdb_with_debug, get_table, vantage_config::EmptyEntity,
    vantage_config::VantageConfig,
};
use clap::{Arg, Command};
use serde_json::Value;
use vantage_core::{error, util::error::Context, Result};

use vantage_surrealdb::prelude::*;
use vantage_table::TableLike;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    // Load config from YAML
    let config = VantageConfig::from_file("bakery_model4/config.yaml")
        .context("Failed to load config.yaml")?;

    let app = Command::new("db")
        .about("Database management utility for Bakery (Dynamic)")
        .arg(
            Arg::new("debug")
                .long("debug")
                .help("Enable debug mode to show SQL queries")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("entity")
                .help("Entity name (e.g., client, product, bakery, order)")
                .required(false),
        )
        .arg(
            Arg::new("commands")
                .help("Commands like: list, get, field=value, etc.")
                .num_args(0..)
                .trailing_var_arg(true),
        );

    let matches = app.get_matches();

    // Check for debug flag
    let debug = matches.get_flag("debug");

    // Connect to database
    let db = connect_surrealdb_with_debug(debug)
        .await
        .context("Failed to connect to SurrealDB")?;

    if let Some(entity_name) = matches.get_one::<String>("entity") {
        let commands: Vec<String> = matches
            .get_many::<String>("commands")
            .unwrap_or_default()
            .cloned()
            .collect();

        let table = get_table(&config, entity_name, db)
            .with_context(|| error!("Failed to get table", entity_name = entity_name))?;

        handle_commands(table, commands)
            .await
            .with_context(|| error!("Failed to handle commands", entity_name = entity_name))?;
    } else {
        // Show available entities
        if let Some(entities) = &config.entities {
            let entity_names: Vec<String> = entities.keys().cloned().collect();
            println!("Available entities: {}", entity_names.join(", "));
        }
        println!("Use 'db <entity> <command>' to interact with data");
        println!("Example: db client list");
        println!("         db client name=Marty list");
        println!("Run 'db --help' for more information");
    }

    Ok(())
}

async fn handle_commands(
    mut table: vantage_table::Table<bakery_model4::vantage_surrealdb::SurrealDB, EmptyEntity>,
    commands: Vec<String>,
) -> Result<()> {
    for command in commands {
        if command.contains('=') {
            let parts: Vec<&str> = command.splitn(2, '=').collect();
            if parts.len() == 2 {
                let field = parts[0];
                let value_str = parts[1];

                // Get column type and parse value accordingly
                if let Some(column) = table.get_column(field) {
                    let col_type = column.get_type();

                    match col_type {
                        "bool" => {
                            let bool_val = matches!(
                                value_str.to_lowercase().as_str(),
                                "true" | "1" | "on" | "yes"
                            );
                            table.add_condition(table[field].eq(bool_val));
                        }
                        "int" => match value_str.parse::<i64>() {
                            Ok(int_val) => {
                                table.add_condition(table[field].eq(int_val));
                            }
                            Err(_) => {
                                println!("❌ Invalid integer value: {}", value_str);
                                continue;
                            }
                        },
                        "float" => match value_str.parse::<f64>() {
                            Ok(float_val) => {
                                table.add_condition(table[field].eq(float_val));
                            }
                            Err(_) => {
                                println!("❌ Invalid float value: {}", value_str);
                                continue;
                            }
                        },
                        _ => {
                            table.add_condition(table[field].eq(value_str.to_string()));
                        }
                    }
                } else {
                    println!("❌ Column '{}' not found", field);
                    continue;
                }
            } else {
                println!("❌ Invalid condition format. Use: field=value");
            }
            continue;
        }

        match command.as_str() {
            "list" => {
                let values = table.get_values().await.context("Failed to get records")?;
                let record_count = values.len();
                let columns = table.columns();
                let display_columns: Vec<(&String, &dyn vantage_table::ColumnLike)> = columns
                    .iter()
                    .take(5)
                    .map(|(k, v)| (k, v as &dyn vantage_table::ColumnLike))
                    .collect();

                let mut table_data = Vec::new();

                for value in values {
                    let mut row = Vec::new();

                    for (col_name, column) in &display_columns {
                        let expected_type = column.get_type();

                        if let Some(v) = value.get(col_name.as_str()) {
                            let (field_value, has_mismatch) = match v {
                                Value::String(s) => {
                                    let mismatch =
                                        expected_type != "string" && expected_type != "any";
                                    (s.clone(), mismatch)
                                }
                                Value::Number(n) => {
                                    let mismatch = expected_type != "int"
                                        && expected_type != "float"
                                        && expected_type != "any";
                                    (n.to_string(), mismatch)
                                }
                                Value::Bool(b) => {
                                    let mismatch =
                                        expected_type != "bool" && expected_type != "any";
                                    (b.to_string(), mismatch)
                                }
                                Value::Null => ("None".to_string(), false),
                                _ => (format!("{:?}", v), expected_type != "any"),
                            };
                            row.push((field_value, has_mismatch));
                        } else {
                            row.push(("None".to_string(), false));
                        }
                    }
                    table_data.push(row);
                }

                if !table_data.is_empty() {
                    let headers: Vec<String> = display_columns
                        .iter()
                        .map(|(name, _)| (*name).clone())
                        .collect();
                    print_table_with_colors(headers, table_data);
                }
                println!("Found {} records", record_count);
            }
            "get" => {
                let values = table.get_values().await.context("Failed to get values")?;
                match values.first() {
                    Some(record) => println!(
                        "{}",
                        serde_json::to_string_pretty(record)
                            .context("Failed to serialize record")?
                    ),
                    None => println!("No record found"),
                }
            }
            _ => {
                println!("Unknown command: {}", command);
                println!("Available commands: list, get");
            }
        }
    }

    Ok(())
}

fn print_table_with_colors(headers: Vec<String>, rows: Vec<Vec<(String, bool)>>) {
    // Calculate column widths based on actual text content
    let mut col_widths = vec![0; headers.len()];
    for (i, header) in headers.iter().enumerate() {
        col_widths[i] = header.len();
    }
    for row in &rows {
        for (i, (cell, _)) in row.iter().enumerate() {
            col_widths[i] = col_widths[i].max(cell.len());
        }
    }

    // Print headers
    for (i, header) in headers.iter().enumerate() {
        if i > 0 {
            print!(" ");
        }
        print!("{:<width$}", header, width = col_widths[i]);
    }
    println!();

    let total_width: usize = col_widths.iter().sum::<usize>() + col_widths.len() - 1;
    println!("{}", "-".repeat(total_width));

    // Print rows with color coding
    for row in &rows {
        for (i, (cell, has_mismatch)) in row.iter().enumerate() {
            if i > 0 {
                print!(" ");
            }
            if *has_mismatch {
                print!(
                    "\x1b[48;5;224m{:<width$}\x1b[0m",
                    cell,
                    width = col_widths[i]
                );
            } else {
                print!("{:<width$}", cell, width = col_widths[i]);
            }
        }
        println!();
    }
}
