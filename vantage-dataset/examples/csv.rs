// examples/csv.rs

mod mocks;
use mocks::csv_mock::{CsvFile, MockCsv};

use serde::{Deserialize, Serialize};
use vantage_dataset::prelude::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct User {
    id: u32,
    name: String,
    email: String,
    age: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Product {
    id: u32,
    name: String,
    price: f64,
    category: String,
}

// Implement DataSetSource for MockCsv
#[async_trait]
impl DataSetSource for MockCsv {
    async fn list_datasets(&self) -> Result<Vec<String>> {
        Ok(self.list_files().cloned().collect())
    }

    async fn get_dataset_info(&self, name: &str) -> Result<Option<serde_json::Value>> {
        if self.get_file_content(name).is_some() {
            Ok(Some(serde_json::json!({
                "name": name,
                "type": "csv",
                "capabilities": ["readable"]
            })))
        } else {
            Ok(None)
        }
    }
}

// Implement generic ReadableDataSetSource for MockCsv
#[async_trait]
impl ReadableDataSetSource for MockCsv {
    type DataSet<E: Entity> = CsvFile<E>;

    async fn get_readable<E: Entity>(&self, name: &str) -> Result<Option<Self::DataSet<E>>> {
        if self.get_file_content(name).is_some() {
            Ok(Some(CsvFile::new(self, name)))
        } else {
            Ok(None)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let csv_ds = MockCsv::new();

    // Test discovery
    println!("=== Dataset Discovery ===");
    let datasets = csv_ds.list_datasets().await?;
    println!("Found datasets: {:?}", datasets);

    for dataset_name in &datasets {
        if let Some(info) = csv_ds.get_dataset_info(dataset_name).await? {
            println!("Dataset '{}': {}", dataset_name, info);
        }
    }

    // Test generic typed dataset access
    println!("\n=== Generic Typed Dataset Access ===");

    // Get users using generic method
    if let Some(users_dataset) = csv_ds.get_readable::<User>("users.csv").await? {
        let all_users: Vec<User> = users_dataset.get().await?;
        println!("All users ({}):", all_users.len());
        for user in &all_users {
            println!(
                "  {} - {} ({}, age {})",
                user.id, user.name, user.email, user.age
            );
        }

        // Read just the first user
        let first_user = users_dataset.get_some().await?.unwrap();
        println!("\nFirst user: {} - {}", first_user.name, first_user.email);

        // Demonstrate partial deserialization
        #[derive(Debug, Deserialize)]
        struct UserContact {
            name: String,
            email: String,
        }

        let contacts: Vec<UserContact> = users_dataset.get_as().await?;
        println!("\nUser contacts only:");
        for contact in contacts {
            println!("  {} - {}", contact.name, contact.email);
        }
    }

    // Get products using generic method
    if let Some(products_dataset) = csv_ds.get_readable::<Product>("products.csv").await? {
        let all_products: Vec<Product> = products_dataset.get().await?;
        println!("\nAll products ({}):", all_products.len());
        for product in &all_products {
            println!(
                "  {} - {} (${}, {})",
                product.id, product.name, product.price, product.category
            );
        }
    }

    // Test direct dataset creation (backward compatibility)
    println!("\n=== Direct Dataset Creation (Backward Compatibility) ===");
    let direct_users = CsvFile::<User>::new(&csv_ds, "users.csv");
    let first_user_direct = direct_users.get_some().await?.unwrap();
    println!(
        "Direct access - First user: {} - {}",
        first_user_direct.name, first_user_direct.email
    );

    // Test error handling
    println!("\n=== Error Handling ===");
    let missing_result = csv_ds.get_readable::<User>("missing.csv").await?;
    match missing_result {
        Some(_) => println!("Unexpected: found missing.csv"),
        None => println!("Correctly returned None for missing.csv"),
    }

    // Try to read from a non-existent file directly
    let missing_file = CsvFile::<User>::new(&csv_ds, "missing.csv");
    match missing_file.get_some().await {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Expected error for missing file: {}", e),
    }

    Ok(())
}
