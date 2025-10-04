//! # Raw ReDB Initialization Example
//!
//! This example shows how to work directly with redb to create initial database
//! and populate it with sample data. This runs before the vantage-redb examples.

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

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Raw ReDB Database Initialization ===");

    // Determine database path - works from both root and vantage-redb directory
    let db_path = if std::env::current_dir()?.file_name().unwrap() == "vantage-redb" {
        PathBuf::from("../db/users.redb")
    } else {
        PathBuf::from("db/users.redb")
    };

    // Create or open the database
    let db = Database::create(&db_path)?;
    println!("✅ Created redb database: {:?}", db_path);

    // Begin write transaction
    let write_txn = db.begin_write()?;
    {
        // Open tables within transaction scope
        let mut users_table = write_txn.open_table(USERS)?;
        let mut email_index = write_txn.open_table(USERS_BY_EMAIL)?;

        // Create sample users
        let users = [
            User {
                name: "Alice Johnson".to_string(),
                email: "alice@example.com".to_string(),
                is_active: true,
                age: 10,
            },
            User {
                name: "Bob Smith".to_string(),
                email: "bob@example.com".to_string(),
                is_active: false,
                age: 25,
            },
            User {
                name: "Charlie Brown".to_string(),
                email: "charlie@example.com".to_string(),
                is_active: true,
                age: 10,
            },
        ];

        // Insert users with serialized data
        for (i, user) in users.iter().enumerate() {
            let user_id = format!("user{}", i + 1);
            let serialized = bincode::serialize(user)?;

            // Insert into main table
            users_table.insert(user_id.as_str(), serialized.as_slice())?;

            // Insert into email index
            email_index.insert(user.email.as_str(), user_id.as_str())?;

            println!("✅ Inserted user: {} ({})", user.name, user.email);
        }
    }
    // Commit transaction
    write_txn.commit()?;
    println!("✅ All data committed to database");

    println!("\n-[ Verifying data with read operations ]------------------------------------");

    // Verify data with read transaction
    let read_txn = db.begin_read()?;
    let users_table = read_txn.open_table(USERS)?;
    let email_index = read_txn.open_table(USERS_BY_EMAIL)?;

    // Read all users
    println!("All users in database:");
    for result in users_table.iter()? {
        let (id, data) = result?;
        let user: User = bincode::deserialize(data.value())?;
        println!(
            "  ID: {}, Name: {}, Email: {}, Active: {}, Age: {}",
            id.value(),
            user.name,
            user.email,
            user.is_active,
            user.age
        );
    }

    // Test email index lookup
    println!("\nTesting email index lookup:");
    if let Some(user_id) = email_index.get("alice@example.com")? {
        let user_data = users_table.get(user_id.value())?.unwrap();
        let user: User = bincode::deserialize(user_data.value())?;
        println!("Found by email: {} -> {}", user.email, user.name);
    }

    println!("\n=== Database initialized successfully ===");
    println!("You can now run: cargo run --example 0-intro");
    println!("The database file 'users.redb' contains sample data for testing.");

    Ok(())
}
