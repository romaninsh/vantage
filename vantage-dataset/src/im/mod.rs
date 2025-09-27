// src/im/mod.rs

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub mod insertable;
pub mod readable;
pub mod table;
pub mod writable;
pub use table::Table;

/// ImDataSource stores tables in memory using IndexMap for ordered iteration
#[derive(Debug, Clone)]
pub struct ImDataSource {
    // table_name -> IndexMap<id, serialized_record>
    tables: Arc<Mutex<HashMap<String, IndexMap<String, serde_json::Value>>>>,
}

impl ImDataSource {
    pub fn new() -> Self {
        Self {
            tables: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_or_create_table(&self, table_name: &str) -> IndexMap<String, serde_json::Value> {
        let mut tables = self.tables.lock().unwrap();
        tables.entry(table_name.to_string()).or_default().clone()
    }

    fn update_table(&self, table_name: &str, table: IndexMap<String, serde_json::Value>) {
        let mut tables = self.tables.lock().unwrap();
        tables.insert(table_name.to_string(), table);
    }
}

impl Default for ImDataSource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        dataset::{InsertableDataSet, ReadableDataSet},
        im::table::Table,
    };

    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct User {
        id: Option<String>,
        name: String,
        email: String,
        age: u32,
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let data_source = ImDataSource::new();
        let users = Table::<User>::new(&data_source, "users");

        // Insert users
        let user1 = User {
            id: Some("user-1".to_string()),
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 30,
        };
        let user2 = User {
            id: None, // This will get auto-generated ID
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            age: 25,
        };

        users.insert(user1.clone()).await.unwrap();
        users.insert(user2.clone()).await.unwrap();

        // Get all users
        let all_users: Vec<User> = users.get().await.unwrap();
        assert_eq!(all_users.len(), 2);

        // Order should be maintained (Alice first, Bob second)
        assert_eq!(all_users[0].name, "Alice");
        assert_eq!(all_users[0].id, Some("user-1".to_string()));
        assert_eq!(all_users[1].name, "Bob");
        assert!(all_users[1].id.is_some()); // Bob should have auto-generated ID
    }

    #[tokio::test]
    async fn test_get_some() {
        let data_source = ImDataSource::new();
        let users = Table::<User>::new(&data_source, "users");

        let user = User {
            id: Some("charlie-123".to_string()),
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
            age: 35,
        };

        users.insert(user.clone()).await.unwrap();

        // Get first user
        let first_user: User = users.get_some().await.unwrap().unwrap();
        assert_eq!(first_user.name, "Charlie");
        assert_eq!(first_user.id, Some("charlie-123".to_string()));
    }

    #[tokio::test]
    async fn test_empty_table() {
        let data_source = ImDataSource::new();
        let users = Table::<User>::new(&data_source, "empty_users");

        // Get from empty table should return empty vec
        let all_users: Vec<User> = users.get().await.unwrap();
        assert_eq!(all_users.len(), 0);

        // Get some from empty table should return error
        let result = users.get_some().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_multiple_tables() {
        let data_source = ImDataSource::new();
        let users = Table::<User>::new(&data_source, "users");

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct Product {
            id: Option<String>,
            name: String,
            price: f64,
        }

        let products = Table::<Product>::new(&data_source, "products");

        // Insert into both tables
        users
            .insert(User {
                id: None,
                name: "Alice".to_string(),
                email: "alice@example.com".to_string(),
                age: 30,
            })
            .await
            .unwrap();

        products
            .insert(Product {
                id: Some("laptop-1".to_string()),
                name: "Laptop".to_string(),
                price: 999.99,
            })
            .await
            .unwrap();

        // Each table should have its own records
        let all_users: Vec<User> = users.get().await.unwrap();
        let all_products: Vec<Product> = products.get().await.unwrap();

        assert_eq!(all_users.len(), 1);
        assert_eq!(all_products.len(), 1);
        assert_eq!(all_users[0].name, "Alice");
        assert!(all_users[0].id.is_some()); // Auto-generated ID
        assert_eq!(all_products[0].name, "Laptop");
        assert_eq!(all_products[0].id, Some("laptop-1".to_string()));
    }

    #[tokio::test]
    async fn test_empty_string_id() {
        let data_source = ImDataSource::new();
        let users = Table::<User>::new(&data_source, "users");

        // Insert user with empty string ID (should be preserved as valid ID)
        let user = User {
            id: Some("".to_string()),
            name: "Empty ID User".to_string(),
            email: "empty@example.com".to_string(),
            age: 25,
        };

        users.insert(user).await.unwrap();

        // Get the user back
        let retrieved_user = users.get_some().await.unwrap().unwrap();
        assert_eq!(retrieved_user.name, "Empty ID User");
        assert_eq!(retrieved_user.id, Some("".to_string())); // Empty string should be preserved
    }

    #[tokio::test]
    async fn test_import() -> Result<(), Box<dyn std::error::Error>> {
        let data_source = ImDataSource::new();
        let users1 = Table::<User>::new(&data_source, "users1");
        let mut users2 = Table::<User>::new(&data_source, "users2");

        // Insert user with empty string ID (should be preserved as valid ID)
        let user = User {
            id: None,
            name: "Empty ID User".to_string(),
            email: "empty@example.com".to_string(),
            age: 25,
        };

        users1.insert(user).await.unwrap();
        let expected_data = users1.get().await?;
        users2.import(users1).await.unwrap();
        assert_eq!(expected_data, users2.get().await?);
        Ok(())
    }
}
