use anyhow::Result;
use bakery_model3::{
    connect_surrealdb_with_debug, get_table, model_names, with_model, BakeryModel,
};
use clap::{Arg, Command};
use serde_json::Value;
use vantage_dataset::prelude::*;
use vantage_surrealdb::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = Command::new("db")
        .about("Database management utility for Bakery")
        .arg(
            Arg::new("debug")
                .long("debug")
                .help("Enable debug mode to show SQL queries")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        );

    // Dynamically add subcommands for each model
    for model_name in model_names() {
        app = app.subcommand(
            Command::new(model_name)
                .about(format!("{} operations", model_name))
                .arg(
                    Arg::new("commands")
                        .help("Commands like: list, get, field=value, etc.")
                        .num_args(0..)
                        .trailing_var_arg(true),
                ),
        );
    }

    let matches = app.get_matches();

    // Check for debug flag
    let debug = matches.get_flag("debug");

    // Connect to database
    connect_surrealdb_with_debug(debug).await?;

    match matches.subcommand() {
        Some((model_name, sub_matches)) => {
            let commands: Vec<String> = sub_matches
                .get_many::<String>("commands")
                .unwrap_or_default()
                .cloned()
                .collect();
            let table = get_table(model_name, bakery_model3::surrealdb())?;
            handle_commands_for(table, commands).await?;
        }
        None => {
            println!("Available models: {}", model_names().join(", "));
            println!("Use 'db <model> <command>' to interact with data");
            println!("Run 'db --help' for more information");
        }
    }

    Ok(())
}

async fn handle_commands_for(model: BakeryModel, commands: Vec<String>) -> Result<()> {
    with_model!(model, handle_commands, commands)
}

async fn handle_commands<E>(
    mut table: vantage_table::Table<bakery_model3::SurrealDB, E>,
    commands: Vec<String>,
) -> Result<()>
where
    E: vantage_table::Entity + std::fmt::Debug + serde::Serialize,
{
    for command in commands {
        if command.contains('=') {
            let parts: Vec<&str> = command.splitn(2, '=').collect();
            if parts.len() == 2 {
                let field = parts[0];
                let value = parts[1];
                table.add_condition(table[field].eq(value.to_string()));
            } else {
                println!("❌ Invalid condition format. Use: field=value");
            }
            continue;
        }

        match command.as_str() {
            "list" => {
                let records = table.get().await?;
                let record_count = records.len();
                let columns = table.columns();
                let display_columns: Vec<&String> =
                    columns.iter().take(5).map(|(k, _)| k).collect();

                let mut table_data = Vec::new();

                for record in records {
                    let value = serde_json::to_value(&record)?;
                    let mut row = Vec::new();

                    for column in &display_columns {
                        let field_value = if let Some(v) = value.get(column.as_str()) {
                            match v {
                                Value::String(s) => s.clone(),
                                Value::Number(n) => n.to_string(),
                                Value::Bool(b) => b.to_string(),
                                Value::Null => "None".to_string(),
                                _ => format!("{:?}", v),
                            }
                        } else {
                            "None".to_string()
                        };
                        row.push(field_value);
                    }
                    table_data.push(row);
                }

                if !table_data.is_empty() {
                    let headers: Vec<String> =
                        display_columns.iter().map(|s| (*s).clone()).collect();
                    print_table(headers, table_data);
                }
                println!("Found {} records", record_count);
            }
            "get" => {
                let record = table.get_some().await?;
                match record {
                    Some(record) => println!("{:?}", record),
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

fn print_table(headers: Vec<String>, rows: Vec<Vec<String>>) {
    let mut all_rows = vec![headers];
    all_rows.extend(rows);

    let mut col_widths = vec![0; all_rows[0].len()];
    for row in &all_rows {
        for (i, cell) in row.iter().enumerate() {
            col_widths[i] = col_widths[i].max(cell.len());
        }
    }

    for (i, header) in all_rows[0].iter().enumerate() {
        if i > 0 {
            print!(" ");
        }
        print!("{:<width$}", header, width = col_widths[i]);
    }
    println!();

    let total_width: usize = col_widths.iter().sum::<usize>() + col_widths.len() - 1;
    println!("{}", "-".repeat(total_width));

    for row in &all_rows[1..] {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                print!(" ");
            }
            print!("{:<width$}", cell, width = col_widths[i]);
        }
        println!();
    }
}
