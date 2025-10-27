use surreal_client::SurrealConnection;
use vantage_config::VantageConfig;
use vantage_surrealdb::SurrealDB;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to SurrealDB
    let dsn = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "ws://root:root@localhost:8000/bakery/v2".to_string());

    let client = SurrealConnection::dsn(&dsn)?.connect().await?;
    let db = SurrealDB::new(client);

    // Load config from YAML file
    let config = VantageConfig::from_file("vantage-config/examples/minimal.yaml")?;

    println!("Loaded config successfully!");
    println!();

    // List all entities
    println!("Available entities:");
    if let Some(entities) = &config.tables {
        for name in entities.keys() {
            println!("  - {}", name);
        }
    }
    println!();

    // Get the client entity
    if let Some(entities) = &config.tables {
        if let Some(client) = entities.get("client") {
            println!("Client entity:");
            println!("  Table: {}", client.table);
            println!();

            println!("  Columns:");
            for column in &client.columns {
                print!("    - {}", column.name);
                if let Some(col_type) = &column.col_type {
                    print!(" (type: {})", col_type);
                }
                if !column.flags.is_empty() {
                    print!(" [flags: {}]", column.flags.join(", "));
                }
                if let Some(default) = &column.default {
                    print!(" [default: {}]", default);
                }
                println!();

                if let Some(rules) = &column.rules {
                    println!("      rules:");
                    for (rule_name, rule_value) in rules {
                        println!("        {}: {}", rule_name, rule_value);
                    }
                }
            }
            println!();
        } else {
            println!("Client table not found!");
        }
    } else {
        println!("No tables defined in config");
    }

    // Test get_table method
    println!("-------------------------------------------------------------------------------");
    println!("Testing get_table():");
    println!();

    if let Some(client_table) = config.get_table("client", db) {
        println!("Successfully created client table from config!");
        println!("  Column count: {}", client_table.columns().len());
    } else {
        println!("Failed to get client table!");
    }

    Ok(())
}
