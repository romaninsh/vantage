use vantage_dataset::AnyCsvType;
use vantage_dataset::traits::ReadableDataSet;
use vantage_types::entity;

use vantage_dataset::mocks::{CsvFile, MockCsv};

#[derive(Debug, Clone, Default)]
#[entity(CsvType)]
struct User {
    id: String,
    name: String,
    email: String,
    age: i64,
}

#[tokio::main]
async fn main() -> vantage_core::Result<()> {
    let csv_ds = MockCsv::new();
    let users = CsvFile::<User>::new(csv_ds, "users.csv");

    // List all users
    let all_users = users.list().await?;
    println!("All users ({})", all_users.len());
    for (id, user) in &all_users {
        println!(
            "  [{}] {} - {} (age {})",
            id, user.name, user.email, user.age
        );
    }

    // Get first user
    if let Some((id, user)) = users.get_some().await? {
        println!("\nFirst user: [{}] {}", id, user.name);
    }

    Ok(())
}
