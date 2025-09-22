// examples/im_example.rs

use serde::{Deserialize, Serialize};
use vantage_dataset::dataset::{InsertableDataSet, ReadableDataSet};
use vantage_dataset::im::{ImDataSource, Table};

mod mocks;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    id: Option<String>,
    name: String,
    email: String,
    age: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Product {
    name: String,
    price: f64,
    category: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let im_data_source = ImDataSource::new();

    let users = Table::<User>::new(&im_data_source, "users");
    let mut products = Table::<Product>::new(&im_data_source, "products");

    // Insert some users
    users
        .insert(User {
            id: Some("user-1".to_string()),
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 30,
        })
        .await?;

    users
        .insert(User {
            id: None, // This will get an auto-generated ID
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            age: 25,
        })
        .await?;

    users
        .insert(User {
            id: Some("charlie-456".to_string()),
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
            age: 35,
        })
        .await?;

    // Insert some products (no ID field needed - auto-generated)
    let csv_products =
        mocks::csv_mock::CsvFile::<Product>::new(&mocks::csv_mock::MockCsv::new(), "products.csv");

    products.import(csv_products).await?;

    // Retrieve and display all users
    let all_users = users.get().await?;
    println!("All users ({}):", all_users.len());
    for user in &all_users {
        let id_display = user.id.as_deref().unwrap_or("<no-id>");
        println!(
            "  [{}] {} - {} (age {})",
            id_display, user.name, user.email, user.age
        );
    }

    // Get first user
    let first_user = users.get_some().await?.unwrap();
    println!("\nFirst user: {}", first_user.name);

    // Retrieve and display all products (no ID field, but still stored with auto-generated IDs)
    let all_products: Vec<Product> = products.get().await?;
    println!("\nAll products ({}):", all_products.len());
    for product in &all_products {
        println!(
            "  {} - ${} ({})",
            product.name, product.price, product.category
        );
    }
    println!("Note: Products have auto-generated IDs internally, but don't expose them");

    // Demonstrate partial deserialization - only get user names and ages
    #[derive(Debug, Deserialize)]
    struct UserSummary {
        name: String,
        age: u32,
    }

    let summaries: Vec<UserSummary> = users.get_as().await?;
    println!("\nUser summaries:");
    for summary in summaries {
        println!("  {} is {} years old", summary.name, summary.age);
    }

    // Show that insertion order is preserved and IDs are properly handled
    println!("\nInsertion order is preserved and IDs are handled:");
    println!(
        "First user inserted: {} (ID: {})",
        all_users[0].name,
        all_users[0].id.as_ref().unwrap()
    ); // Should be Alice with user-1
    println!(
        "Second user inserted: {} (ID: {})",
        all_users[1].name,
        all_users[1].id.as_ref().unwrap()
    ); // Should be Bob with auto-generated ID
    println!(
        "Third user inserted: {} (ID: {})",
        all_users[2].name,
        all_users[2].id.as_ref().unwrap()
    ); // Should be Charlie with charlie-456

    // Demonstrate overwriting by inserting user with existing ID
    users
        .insert(User {
            id: Some("user-1".to_string()), // Same ID as Alice - should overwrite
            name: "Alice Updated".to_string(),
            email: "alice.new@example.com".to_string(),
            age: 31,
        })
        .await?;

    let updated_users: Vec<User> = users.get().await?;
    println!("\nAfter overwriting user-1:");
    println!("Total users: {} (should still be 3)", updated_users.len());
    for user in &updated_users {
        if user.id.as_ref().unwrap() == "user-1" {
            println!("  Overwritten user: {} - {}", user.name, user.email);
        }
    }

    Ok(())
}
