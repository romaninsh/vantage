use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use vantage_dataset::dataset::ReadableDataSet;
use vantage_redb::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct User {
    name: String,
    email: String,
    is_active: bool,
    age: u32,
}

impl User {
    pub fn table() -> vantage_table::Table<Redb, User> {
        // Determine database path - works from both root and vantage-redb directory
        let db_path = if std::env::current_dir().unwrap().file_name().unwrap() == "vantage-redb" {
            PathBuf::from("../db/users.redb")
        } else {
            PathBuf::from("db/users.redb")
        };

        let db = Redb::open(&db_path).expect("Failed to open database");
        vantage_table::Table::new("users", db)
            .with_column("age")
            .into_entity()
    }

    pub fn select() -> vantage_redb::RedbSelect<User> {
        vantage_redb::RedbSelect::new()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Vantage ReDB Introduction Example ===");

    let users_table = User::table();
    let users = users_table.get().await?;
    for user in users {
        println!("  - {} ({}, age {})", user.name, user.email, user.age);
    }

    // println!("\n-[ rebuilding age index ]------------------------------------");
    // // Rebuild the age column index for fast lookups by age value
    // if let Some(age_column) = users_table.column("age") {
    //     println!("Rebuilding index for age column...");

    //     // Get write transaction for index operations
    //     let write_txn = users_table
    //         .data_source()
    //         .begin_write()
    //         .expect("Failed to begin write transaction");

    //     match age_column.rebuild_index(&users_table, &write_txn).await {
    //         Ok(()) => {
    //             write_txn.commit().expect("Failed to commit index rebuild");
    //             println!("✅ Age index rebuilt successfully");
    //         }
    //         Err(e) => println!("❌ Index rebuild failed: {}", e),
    //     }
    // } else {
    //     println!("❌ Age column not found");
    // }

    println!("\n=== ReDB Key-Value Store Features ===");
    println!("✅ ACID transactions - All operations are atomic");
    println!("✅ Key-value storage - Direct access by ID");
    println!("✅ Secondary indexes - Fast lookups by column values");
    println!("✅ Embedded database - No server required");
    println!("\nNote: Run 'cargo run --example 0-init' first to create sample data");

    Ok(())
}
