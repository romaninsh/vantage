use schemars::schema_for;
use std::fs;
use std::process;
use vantage_config::VantageConfig;

fn main() {
    let schema = schema_for!(VantageConfig);

    let json = match serde_json::to_string_pretty(&schema) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("Error: Failed to serialize schema: {}", e);
            process::exit(1);
        }
    };

    let filename = "vantage-config.schema.json";

    match fs::write(filename, json) {
        Ok(_) => {
            println!("Generated schema: {}", filename);
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Error: Failed to write schema file: {}", e);
            process::exit(1);
        }
    }
}
