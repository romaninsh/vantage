//! Demonstrates datasource-agnostic code: the same export_jsonl() function
//! works regardless of the underlying data source.
//!
//! Usage:
//!   cargo run --example agnostic_jsonl_exporter -- csv path/to/file.csv
//!   cargo run --example agnostic_jsonl_exporter -- surreal    (TODO)
//!   cargo run --example agnostic_jsonl_exporter -- postgres   (TODO)
//!   cargo run --example agnostic_jsonl_exporter -- api        (TODO)

use std::fmt::Display;

use clap::{Parser, Subcommand};
use serde_json::Value;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;

use example_for_website::sources::CsvSource;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    source: Source,
}

#[derive(Subcommand)]
enum Source {
    /// Read from a CSV file
    Csv { path: String },
    /// Read from SurrealDB (not yet implemented)
    Surreal,
    /// Read from PostgreSQL (not yet implemented)
    Postgres,
    /// Read from a REST API (not yet implemented)
    Api,
}

// ---------------------------------------------------------------------------
// The agnostic export function — knows nothing about the data source
// ---------------------------------------------------------------------------

async fn export_jsonl<DS, F>(ds: &DS, to_json: F) -> Result<String>
where
    DS: ReadableValueSet,
    DS::Id: Display,
    F: Fn(DS::Value) -> Value,
{
    let records = ds.list_values().await?;

    let lines: Vec<String> = records
        .into_iter()
        .map(|(id, record)| {
            let mut obj = serde_json::Map::new();
            obj.insert("_id".to_string(), Value::String(id.to_string()));
            for (key, val) in record {
                obj.insert(key, to_json(val));
            }
            Value::Object(obj).to_string()
        })
        .collect();

    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args = Args::parse();

    let output = match args.source {
        Source::Csv { path } => {
            let source = CsvSource::new(path);
            export_jsonl(&source, |v| v).await?
        }
        Source::Surreal => {
            println!("SurrealDB source not yet implemented");
            return Ok(());
        }
        Source::Postgres => {
            println!("PostgreSQL source not yet implemented (vantage-postgres pending)");
            return Ok(());
        }
        Source::Api => {
            println!("REST API source not yet implemented");
            return Ok(());
        }
    };

    println!("{}", output);
    Ok(())
}
