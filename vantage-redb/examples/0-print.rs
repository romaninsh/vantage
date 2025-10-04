//! # Print ReDB Database Contents
//!
//! Simple utility to print all contents from users.redb database.
//! Run after 0-init to see the stored data.

use anyhow::Result;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct User {
    name: String,
    email: String,
    is_active: bool,
    age: u32,
}

// Define table schema for redb
const USERS: TableDefinition<&str, &[u8]> = TableDefinition::new("users");
const USERS_BY_EMAIL: TableDefinition<&str, &str> = TableDefinition::new("users_by_email");
const USERS_IDX_AGE: TableDefinition<&str, &[u8]> = TableDefinition::new("users_idx_age");

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== ReDB Database Contents ===");

    // Determine database path - works from both root and vantage-redb directory
    let db_path = if std::env::current_dir()?.file_name().unwrap() == "vantage-redb" {
        PathBuf::from("../db/users.redb")
    } else {
        PathBuf::from("db/users.redb")
    };

    // Open the database (read-only)
    let db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(_) => {
            println!("❌ Database '{:?}' not found.", db_path);
            println!("Run 'cargo run --example 0-init' first to create the database.");
            return Ok(());
        }
    };

    println!("✅ Opened database: {:?}", db_path);

    // Begin read transaction
    let read_txn = db.begin_read()?;
    let users_table = read_txn.open_table(USERS)?;
    let email_index = read_txn.open_table(USERS_BY_EMAIL)?;
    let age_index = read_txn.open_table(USERS_IDX_AGE)?;

    println!("\n-[ Main Users Table ]------------------------------------");
    let mut user_count = 0;
    for result in users_table.iter()? {
        let (id, data) = result?;
        let user: User = bincode::deserialize(data.value())?;
        println!(
            "  {} -> {} ({}, age {}, active: {})",
            id.value(),
            user.name,
            user.email,
            user.age,
            user.is_active
        );
        user_count += 1;
    }
    println!("Total users: {}", user_count);

    println!("\n-[ Email Index Table ]------------------------------------");
    let mut index_count = 0;
    for result in email_index.iter()? {
        let (email, user_id) = result?;
        println!("  {} -> {}", email.value(), user_id.value());
        index_count += 1;
    }
    println!("Total email indexes: {}", index_count);

    println!("\n-[ Age Index Table ]------------------------------------");
    let mut age_index_count = 0;
    for result in age_index.iter()? {
        let (age_value, ids_data) = result?;
        let ids: Vec<String> = bincode::deserialize(ids_data.value())?;
        println!("  age {} -> {:?}", age_value.value(), ids);
        age_index_count += 1;
    }
    println!("Total age indexes: {}", age_index_count);

    Ok(())
}
