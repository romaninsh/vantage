use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vantage_dataset::prelude::InsertableDataSet;
use vantage_redb::prelude::*;
use vantage_redb::util::Result;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct User {
    name: String,
    email: String,
    is_active: bool,
    age: u32,
}

impl User {
    pub fn table() -> vantage_table::Table<Redb, User> {
        let db_path = if std::env::current_dir().unwrap().file_name().unwrap() == "vantage-redb" {
            PathBuf::from("../db/users.redb")
        } else {
            PathBuf::from("db/users.redb")
        };

        let db = Redb::open(&db_path).expect("Failed to open database");
        vantage_table::Table::new("users", db)
            .with_column("email")
            .into_entity()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Testing ReDB Index Rebuild ===");

    let users_table = User::table();

    println!("✅ Connected to database");

    // Test rebuilding email index
    println!("\n-[ Rebuilding email index ]------------------------------------");
    match users_table
        .data_source()
        .redb_rebuild_index(&users_table, "email")
        .await
    {
        Ok(()) => println!("✅ Email index rebuilt successfully"),
        Err(e) => println!("❌ Email index rebuild failed: {}", e),
    }

    // Test rebuilding age index
    println!("\n-[ Rebuilding age index ]------------------------------------");
    match users_table
        .data_source()
        .redb_rebuild_index(&users_table, "age")
        .await
    {
        Ok(()) => println!("✅ Age index rebuilt successfully"),
        Err(e) => println!("❌ Age index rebuild failed: {}", e),
    }

    // Test insert operation that should trigger automatic rebuild on failure
    println!("\n-[ Testing insert with automatic rebuild ]------------------");
    let new_user = User {
        name: "Test User".to_string(),
        email: "test@rebuild.com".to_string(),
        is_active: true,
        age: 30,
    };

    match users_table.insert(new_user).await {
        Ok(id) => println!("✅ Insert successful, ID: {:?}", id),
        Err(e) => println!("❌ Insert failed: {}", e),
    }

    println!("\n=== Index Rebuild Test Complete ===");
    Ok(())
}
