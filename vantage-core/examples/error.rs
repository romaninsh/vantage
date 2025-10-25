//! Example demonstrating the VantageError with context tracking
//!
//! Run with: cargo run --example error -p vantage-core --features colored-errors

use vantage_core::{Context, VantageError, error};

// Demo functions
fn read_database(_table: &str) -> Result<String, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Connection refused",
    ))
}

fn fetch_client_data(client_id: i32) -> Result<String, VantageError> {
    let table = "client";
    read_database(table).context(error!(
        "Failed to read from database",
        table = table,
        client_id = client_id
    ))
}

fn process_user_request(user: &str, client_id: i32) -> Result<String, VantageError> {
    fetch_client_data(client_id).context(error!(
        "Failed to process user request",
        user = user,
        operation = "fetch_client"
    ))
}

fn main() {
    use std::process::Termination;

    println!("=== VantageError, default compact output ===\n");

    // Simulate an error chain
    let result = process_user_request("john", 42);

    match result {
        Ok(data) => println!("Success: {}", data),
        Err(e) => {
            eprintln!("Error: {}\n", e);
        }
    }

    println!("=== VantageError, formatted output ===");

    let result = process_user_request("john", 42);

    match result {
        Ok(data) => println!("Success: {}", data),
        Err(e) => {
            e.report();
        }
    };

    println!("\n=== Backward Compatible Usage ===\n");

    // Old style still works
    let result: Result<(), VantageError> = Err("Simple error message".into());
    match result {
        Ok(_) => println!("Success"),
        Err(e) => {
            e.report();
            println!();
        }
    }

    println!("\n=== Simple Context ===\n");

    // Simple context without location
    let result: Result<String, VantageError> =
        read_database("products").context("Failed to read products");
    match result {
        Ok(_) => println!("Success"),
        Err(e) => {
            e.report();
            println!();
        }
    }

    println!("\n=== 4-Level Error Depth ===\n");

    fn level4() -> Result<(), VantageError> {
        Err(error!("Deepest error", depth = 4, component = "database"))
    }

    fn level3b() -> Result<(), VantageError> {
        level4().context(error!("Connection error", depth = 3, pool = "primary"))
    }

    fn level2b() -> Result<(), VantageError> {
        level3b().context(error!("Query failed", depth = 2, table = "users"))
    }

    fn level1b() -> Result<(), VantageError> {
        level2b().context(error!("API request failed", depth = 1, endpoint = "/users"))
    }

    match level1b() {
        Ok(_) => println!("Success"),
        Err(e) => {
            use std::process::Termination;
            e.report();
            println!();
        }
    }

    #[cfg(feature = "colored-errors")]
    println!("\n✓ Colored output is enabled");
    #[cfg(not(feature = "colored-errors"))]
    println!("\n○ Colored output is disabled (run with --features colored-errors to enable)");
}
