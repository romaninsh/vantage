use serde::{Deserialize, Serialize};

use vantage_dataset::{
    im::{ImDataSource, ImTable},
    traits::{InsertableDataSet, ReadableDataSet, WritableDataSet},
};
use vantage_types::persistence_serde;

// Simple test entities with serde Record conversion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[persistence_serde]
struct User {
    id: Option<String>,
    name: String,
    age: i32,
    active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[persistence_serde]
struct Product {
    id: Option<String>,
    name: String,
    price: i32,
    available: bool,
}

#[tokio::test]
async fn test_readable_dataset() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    // Test empty list
    let result = table.list().await.unwrap();
    assert_eq!(result.len(), 0);

    // Test get non-existent record
    let result = table.get(&"nonexistent".to_string()).await;
    assert!(result.is_err());

    // Test get_some on empty dataset
    let result = table.get_some().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_insertable_dataset() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    // Test insert with no ID - should generate one
    let user = User {
        id: None,
        name: "Alice".to_string(),
        age: 30,
        active: true,
    };
    let id = table.insert_return_id(user.clone()).await.unwrap();
    assert!(!id.is_empty());

    // Test insert with existing ID - should use it
    let user_with_id = User {
        id: Some("user-123".to_string()),
        name: "Bob".to_string(),
        age: 25,
        active: false,
    };
    let id = table.insert_return_id(user_with_id).await.unwrap();
    assert_eq!(id, "user-123");

    // Test insert with empty ID - should generate one
    let user_empty_id = User {
        id: Some("".to_string()),
        name: "Charlie".to_string(),
        age: 35,
        active: true,
    };
    let id = table.insert_return_id(user_empty_id).await.unwrap();
    assert!(!id.is_empty());
    assert_ne!(id, "");
}

#[tokio::test]
async fn test_writable_dataset() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    let user = User {
        id: Some("user-1".to_string()),
        name: "Alice".to_string(),
        age: 30,
        active: true,
    };

    // Test insert (idempotent)
    let result = table
        .insert(&"user-1".to_string(), user.clone())
        .await
        .unwrap();
    assert_eq!(result, user);

    // Test insert again - should return existing
    let user2 = User {
        id: Some("user-1".to_string()),
        name: "Different Name".to_string(),
        age: 25,
        active: false,
    };
    let result = table.insert(&"user-1".to_string(), user2).await.unwrap();
    assert_eq!(result, user); // Should return original, not new data

    // Test replace
    let updated_user = User {
        id: Some("user-1".to_string()),
        name: "Alice Updated".to_string(),
        age: 31,
        active: false,
    };
    let result = table
        .replace(&"user-1".to_string(), updated_user.clone())
        .await
        .unwrap();
    assert_eq!(result, updated_user);

    // Test patch
    let patch_user = User {
        id: Some("user-1".to_string()),
        name: "Alice Patched".to_string(),
        age: 32,
        active: true,
    };
    let result = table
        .patch(&"user-1".to_string(), patch_user)
        .await
        .unwrap();
    assert_eq!(result.name, "Alice Patched");
    assert_eq!(result.age, 32);
    assert_eq!(result.active, true);

    // Test delete
    table.delete(&"user-1".to_string()).await.unwrap();
    let result = table.get(&"user-1".to_string()).await;
    assert!(result.is_err());

    // Test delete_all
    let _ = table
        .insert(&"user-2".to_string(), user.clone())
        .await
        .unwrap();
    let _ = table
        .insert(&"user-3".to_string(), user.clone())
        .await
        .unwrap();

    table.delete_all().await.unwrap();
    let result = table.list().await.unwrap();
    assert_eq!(result.len(), 0);
}

#[tokio::test]
async fn test_full_crud_cycle() {
    let ds = ImDataSource::new();
    let table = ImTable::<Product>::new(&ds, "products");

    // Create
    let product = Product {
        id: Some("prod-1".to_string()),
        name: "Laptop".to_string(),
        price: 1200,
        available: true,
    };
    let result = table
        .insert(&"prod-1".to_string(), product.clone())
        .await
        .unwrap();
    assert_eq!(result, product);

    // Read
    let retrieved = table.get(&"prod-1".to_string()).await.unwrap();
    assert_eq!(retrieved, product);

    // Update
    let updated_product = Product {
        id: Some("prod-1".to_string()),
        name: "Gaming Laptop".to_string(),
        price: 1500,
        available: true,
    };
    let result = table
        .replace(&"prod-1".to_string(), updated_product.clone())
        .await
        .unwrap();
    assert_eq!(result, updated_product);

    // Verify update
    let retrieved = table.get(&"prod-1".to_string()).await.unwrap();
    assert_eq!(retrieved.name, "Gaming Laptop");
    assert_eq!(retrieved.price, 1500);

    // Delete
    table.delete(&"prod-1".to_string()).await.unwrap();
    let result = table.get(&"prod-1".to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_record_field_handling() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    // Test that ID field is properly handled
    let user = User {
        id: Some("test-user".to_string()),
        name: "Test User".to_string(),
        age: 42,
        active: true,
    };

    // Insert user
    table
        .insert(&"test-user".to_string(), user.clone())
        .await
        .unwrap();

    // Retrieve and verify ID is properly restored
    let retrieved = table.get(&"test-user".to_string()).await.unwrap();
    assert_eq!(retrieved.id, Some("test-user".to_string()));
    assert_eq!(retrieved.name, "Test User");
    assert_eq!(retrieved.age, 42);
    assert_eq!(retrieved.active, true);

    // Test patch doesn't affect ID
    let patch = User {
        id: Some("different-id".to_string()), // This should be ignored in patch logic
        name: "Patched Name".to_string(),
        age: 43,
        active: false,
    };

    let patched = table.patch(&"test-user".to_string(), patch).await.unwrap();
    assert_eq!(patched.id, Some("test-user".to_string())); // ID should remain unchanged
    assert_eq!(patched.name, "Patched Name");
    assert_eq!(patched.age, 43);
    assert_eq!(patched.active, false);
}

#[tokio::test]
async fn test_error_conditions() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    // Test patch on non-existent record
    let user = User {
        id: Some("nonexistent".to_string()),
        name: "Test".to_string(),
        age: 30,
        active: true,
    };
    let result = table.patch(&"nonexistent".to_string(), user).await;
    assert!(result.is_err());

    // Test get on non-existent record
    let result = table.get(&"nonexistent".to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multiple_tables() {
    let ds = ImDataSource::new();
    let user_table = ImTable::<User>::new(&ds, "users");
    let product_table = ImTable::<Product>::new(&ds, "products");

    // Insert into different tables
    let user = User {
        id: Some("user-1".to_string()),
        name: "Alice".to_string(),
        age: 30,
        active: true,
    };

    let product = Product {
        id: Some("prod-1".to_string()),
        name: "Laptop".to_string(),
        price: 1200,
        available: true,
    };

    user_table
        .insert(&"user-1".to_string(), user.clone())
        .await
        .unwrap();
    product_table
        .insert(&"prod-1".to_string(), product.clone())
        .await
        .unwrap();

    // Verify isolation
    let users = user_table.list().await.unwrap();
    let products = product_table.list().await.unwrap();

    assert_eq!(users.len(), 1);
    assert_eq!(products.len(), 1);

    // Test that deleting from one table doesn't affect the other
    user_table.delete_all().await.unwrap();

    let users_after = user_table.list().await.unwrap();
    let products_after = product_table.list().await.unwrap();

    assert_eq!(users_after.len(), 0);
    assert_eq!(products_after.len(), 1); // Should be unaffected
}
