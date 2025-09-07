// examples/csv.rs

mod mocks;
use mocks::csv_mock::{CsvFile, MockCsv};

use serde::{Deserialize, Serialize};
use vantage_dataset::dataset::ReadableDataSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    id: u32,
    name: String,
    email: String,
    age: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Product {
    id: u32,
    name: String,
    price: f64,
    category: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let csv_ds = MockCsv::new();

    let users = CsvFile::<User>::new(&csv_ds, "users.csv");
    let products = CsvFile::<Product>::new(&csv_ds, "products.csv");

    // Read all users
    let all_users: Vec<User> = users.get().await?;
    println!("All users ({}):", all_users.len());
    for user in &all_users {
        println!(
            "  {} - {} ({}, age {})",
            user.id, user.name, user.email, user.age
        );
    }

    // Read just the first user
    let first_user = users.get_some().await?.unwrap();
    println!("\nFirst user: {} - {}", first_user.name, first_user.email);

    // Read all products
    let all_products: Vec<Product> = products.get().await?;
    println!("\nAll products ({}):", all_products.len());
    for product in &all_products {
        println!(
            "  {} - {} (${}, {})",
            product.id, product.name, product.price, product.category
        );
    }

    // Demonstrate partial deserialization - only get user names and emails
    #[derive(Debug, Deserialize)]
    struct UserContact {
        name: String,
        email: String,
    }

    let contacts: Vec<UserContact> = users.get_as().await?;
    println!("\nUser contacts only:");
    for contact in contacts {
        println!("  {} - {}", contact.name, contact.email);
    }

    // Try to read a non-existent file
    let missing_file = CsvFile::<User>::new(&csv_ds, "missing.csv");
    match missing_file.get_some().await {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("\nExpected error for missing file: {}", e),
    }

    Ok(())
}
