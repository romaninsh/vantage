use serde::{Deserialize, Serialize};
use vantage_dataset::dataset::ReadableDataSet;

mod mocks;
use mocks::csv_mock::{CsvFile, MockCsv};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct User {
    id: u32,
    name: String,
    email: String,
    age: u32,
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
