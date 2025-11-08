//! # RedbSelect Example
//!
//! Basic RedbSelect usage with conditions and ordering.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vantage_expressions::protocol::datasource::SelectSource;
use vantage_redb::util::Result;
use vantage_redb::{Redb, RedbSelect};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct User {
    name: String,
    email: String,
    is_active: bool,
    age: u32,
}

impl User {
    pub fn select() -> vantage_redb::RedbSelect<User> {
        vantage_redb::RedbSelect::new()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== RedbSelect Example ===");

    // Determine database path
    let db_path = if std::env::current_dir()?.file_name().unwrap() == "vantage-redb" {
        PathBuf::from("../db/users.redb")
    } else {
        PathBuf::from("db/users.redb")
    };

    if !db_path.exists() {
        println!("Database not found. Run: cargo run --example 0-init");
        return Ok(());
    }

    let db = Redb::open(&db_path)?;

    // All users - using User::select() method
    let select = User::select();
    let results = db.execute_select(&select).await?;
    println!("All users:\n{}\n", serde_json::to_string_pretty(&results)?);

    // Filter by age=10 - using direct RedbSelect creation
    let select_filtered = RedbSelect::<User>::new().with_condition("age", serde_json::json!(10));
    let filtered_results = db.execute_select(&select_filtered).await?;
    println!(
        "Filtered by age=10:\n{}\n",
        serde_json::to_string_pretty(&filtered_results)?
    );

    // Order by name ascending - using User::select() method
    let select_ordered = User::select().with_order("name", true);
    let ordered_results = db.execute_select(&select_ordered).await?;
    println!(
        "Ordered by name:\n{}\n",
        serde_json::to_string_pretty(&ordered_results)?
    );

    // Order by age descending - using User::select() method
    let select_age_desc = User::select().with_order("age", false);
    let age_desc_results = db.execute_select(&select_age_desc).await?;
    println!(
        "Ordered by age (descending):\n{}\n",
        serde_json::to_string_pretty(&age_desc_results)?
    );

    // Limit to first 2 users - using User::select() method
    let select_limited = User::select().with_limit(2);
    let limited_results = db.execute_select(&select_limited).await?;
    println!(
        "Limited to 2 results:\n{}\n",
        serde_json::to_string_pretty(&limited_results)?
    );

    // Limit with ordering - using User::select() method
    let select_ordered_limited = User::select().with_order("name", true).with_limit(2);
    let ordered_limited_results = db.execute_select(&select_ordered_limited).await?;
    println!(
        "Ordered by name, limited to 2:\n{}\n",
        serde_json::to_string_pretty(&ordered_limited_results)?
    );

    Ok(())
}
