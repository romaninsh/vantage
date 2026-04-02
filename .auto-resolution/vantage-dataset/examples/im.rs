use serde::{Deserialize, Serialize};
use vantage_dataset::im::{ImDataSource, ImTable};
use vantage_dataset::traits::{InsertableDataSet, ReadableDataSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct User {
    id: Option<String>,
    name: String,
    email: String,
    age: u32,
}

#[tokio::main]
async fn main() -> vantage_core::Result<()> {
    let im_data_source = ImDataSource::new();
    let users = ImTable::<User>::new(&im_data_source, "users");

    // Insert some users
    let user1_id = users
        .insert_return_id(&User {
            id: Some("user-1".to_string()),
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 30,
        })
        .await?;

    let user2_id = users
        .insert_return_id(&User {
            id: None,
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            age: 25,
        })
        .await?;

    println!("Inserted Alice with ID: {}", user1_id);
    println!("Inserted Bob with ID: {}", user2_id);

    // List all users
    let all_users = users.list().await?;
    println!("\nAll users ({})", all_users.len());
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
