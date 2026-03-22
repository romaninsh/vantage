use bakery_model3::*;
use clap::{Arg, Command};
use vantage_cli_util::print_table;
use vantage_csv::Csv;
use vantage_dataset::prelude::*;
use vantage_table::table::Table;

fn model_names() -> Vec<&'static str> {
    vec!["bakery", "client", "product", "order"]
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

async fn run() -> vantage_core::Result<()> {
    let mut app = Command::new("db")
        .about("Database management utility for Bakery (CSV)")
        .arg(
            Arg::new("data")
                .long("data")
                .help("Path to CSV data directory")
                .default_value("bakery_model3/data"),
        );

    for model_name in model_names() {
        app = app.subcommand(
            Command::new(model_name)
                .about(format!("{} operations", model_name))
                .arg(
                    Arg::new("commands")
                        .help("Commands like: list, get")
                        .num_args(0..)
                        .trailing_var_arg(true),
                ),
        );
    }

    let matches = app.get_matches();
    let data_dir = matches.get_one::<String>("data").unwrap();
    let csv = Csv::new(data_dir.as_str());

    match matches.subcommand() {
        Some((model_name, sub_matches)) => {
            let commands: Vec<String> = sub_matches
                .get_many::<String>("commands")
                .unwrap_or_default()
                .cloned()
                .collect();

            match model_name {
                "bakery" => handle_commands(Bakery::csv_table(csv.clone()), commands).await?,
                "client" => handle_commands(Client::csv_table(csv.clone()), commands).await?,
                "product" => handle_commands(Product::csv_table(csv.clone()), commands).await?,
                "order" => handle_commands(Order::csv_table(csv.clone()), commands).await?,
                _ => println!("Unknown model: {}", model_name),
            }
        }
        None => {
            println!("Available models: {}", model_names().join(", "));
            println!("Use 'db <model> <command>' to interact with data");
            println!("Run 'db --help' for more information");
        }
    }

    Ok(())
}

async fn handle_commands<E>(
    table: Table<Csv, E>,
    commands: Vec<String>,
) -> vantage_core::Result<()>
where
    E: vantage_types::Entity<AnyCsvType> + std::fmt::Debug,
{
    for command in &commands {
        match command.as_str() {
            "list" => {
                let records = table.list_values().await?;
                print_table(&records);
            }
            "get" => {
                let result = table.get_some().await?;
                match result {
                    Some((_id, entity)) => println!("{:?}", entity),
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
