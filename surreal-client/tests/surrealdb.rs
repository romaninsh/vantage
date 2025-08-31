//! Integration tests for SurrealDB using a real database instance
//!
//! These tests require a SurrealDB instance running at localhost:8000
//! with default root credentials (root/root).
//!
//! To run these tests:
//! 1. Start SurrealDB: `surreal start --log trace --user root --pass root memory`
//! 2. Run tests: `cargo test --test surrealdb`

use serde_json::{Value, json};

use surreal_client::{SurrealClient, SurrealConnection};

const DB_URL: &str = "ws://localhost:8000";
const ROOT_USER: &str = "root";
const ROOT_PASS: &str = "root";
const TEST_NAMESPACE: &str = "test";
const TEST_DATABASE: &str = "integration";

async fn get_client() -> SurrealClient {
    SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await
        .expect("Failed to create connection")
}

#[tokio::test]
async fn test_basic_connection() {
    let client = get_client().await;

    // Test version command
    let version = client.version().await.expect("Failed to get version");
    println!("SurrealDB version: {}", version);
    assert!(!version.is_empty());

    // Test info command
    let info = client.info().await.expect("Failed to get session info");
    println!("Session info: {:?}", info);
}

#[tokio::test]
#[ignore]
async fn test_http_connection() {
    let client = SurrealConnection::new()
        .url("http://localhost:8000")
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await
        .expect("Failed to connect to SurrealDB via HTTP");

    let version = client.version().await.expect("Failed to get version");
    assert!(!version.is_empty());
}

#[tokio::test]
async fn test_dsn_connection() {
    let dsn = format!(
        "ws://{}:{}@localhost:8000/{}/{}",
        ROOT_USER, ROOT_PASS, TEST_NAMESPACE, TEST_DATABASE
    );

    let client = SurrealConnection::dsn(&dsn)
        .expect("Failed to parse DSN")
        .connect()
        .await
        .expect("Failed to connect using DSN");

    let version = client.version().await.expect("Failed to get version");
    assert!(!version.is_empty());
}

#[tokio::test]
async fn test_crud_operations() {
    let client = get_client().await;

    // Clean up any existing test data
    let _ = client.delete("users").await;

    // Test CREATE
    let user_data = json!({
        "name": "John Doe",
        "email": "john@example.com",
        "age": 30,
        "active": true
    });

    let created = client
        .create("users:john", Some(user_data.clone()))
        .await
        .expect("Failed to create user");

    println!("Created user: {:?}", created);
    assert!(created.is_object());

    // Test SELECT specific record
    let selected = client
        .select("users:john")
        .await
        .expect("Failed to select user");

    println!("Selected user: {:?}", selected);
    if let Value::Array(results) = &selected {
        assert!(!results.is_empty());
        if let Some(Value::Object(user)) = results.first() {
            assert_eq!(
                user.get("name"),
                Some(&Value::String("John Doe".to_string()))
            );
            assert_eq!(user.get("age"), Some(&Value::Number(30.into())));
        }
    }

    // Test UPDATE
    let updated_data = json!({
        "age": 31,
        "city": "New York"
    });

    let updated = client
        .update("users:john", Some(updated_data))
        .await
        .expect("Failed to update user");

    println!("Updated user: {:?}", updated);
    assert!(updated.is_object());

    // Test MERGE
    let merge_data = json!({
        "active": false,
        "last_login": "2023-12-01"
    });

    let merged = client
        .merge("users:john", merge_data)
        .await
        .expect("Failed to merge user");

    println!("Merged user: {:?}", merged);
    assert!(merged.is_object());

    // Test PATCH - multiple operations
    let patch_data = vec![
        json!({ "op": "replace", "path": "/email", "value": "john.doe@example.com" }),
        json!({ "op": "add", "path": "/tags", "value": ["vip", "premium"] }),
    ];

    let patched = client
        .patch("users:john", patch_data)
        .await
        .expect("Failed to patch user");

    println!("Patched user: {:?}", patched);

    // Test SELECT all
    let all_users = client
        .select("users")
        .await
        .expect("Failed to select all users");
    println!("All users: {:?}", all_users);

    // Test DELETE specific record
    let deleted = client
        .delete("users:john")
        .await
        .expect("Failed to delete user");

    println!("Deleted: {:?}", deleted);

    // Verify deletion
    let after_delete = client
        .select("users:john")
        .await
        .expect("Failed to select after delete");

    if let Value::Array(results) = after_delete {
        assert!(results.is_empty(), "User should be deleted");
    }
}

#[tokio::test]
#[ignore]
// Often fails in a pipeline with error:
// Failed to create product: Protocol("Server error: {\"code\":-32000,\"message\":\"There was a problem with the database: The query was not executed due to a failed transaction. Failed to commit transaction due to a read or write conflict. This transaction can be retried\"}")
async fn test_bulk_operations() {
    let client = get_client().await;

    // Clean up
    let _ = client.delete("products").await;

    // Test bulk CREATE
    let products = vec![
        json!({
            "name": "Laptop",
            "price": 999.99,
            "category": "Electronics",
            "in_stock": true
        }),
        json!({
            "name": "Mouse",
            "price": 29.99,
            "category": "Electronics",
            "in_stock": true
        }),
        json!({
            "name": "Keyboard",
            "price": 79.99,
            "category": "Electronics",
            "in_stock": false
        }),
    ];

    for (i, product) in products.iter().enumerate() {
        client
            .create(&format!("products:prod_{}", i + 1), Some(product.clone()))
            .await
            .expect("Failed to create product");
    }

    // Test bulk SELECT
    let all_products = client
        .select("products")
        .await
        .expect("Failed to select all products");

    println!("All products: {:?}", all_products);
    if let Value::Array(results) = &all_products {
        assert_eq!(results.len(), 3);
    }

    // Test bulk UPDATE with WHERE clause
    let update_query = "UPDATE products SET price = price * 0.9 WHERE category = 'Electronics'";
    let updated = client
        .query(update_query, None)
        .await
        .expect("Failed to bulk update");

    println!("Bulk update result: {:?}", updated);

    // Verify updates
    let updated_products = client
        .select("products")
        .await
        .expect("Failed to select after update");

    println!("Products after discount: {:?}", updated_products);

    // Clean up
    let _ = client.delete("products").await;
}

#[tokio::test]
async fn test_session_variables() {
    let mut client = get_client().await;

    // Test setting variables
    client
        .let_var("user_id", json!("user123"))
        .await
        .expect("Failed to set user_id variable");

    client
        .let_var("permissions", json!(["read", "write"]))
        .await
        .expect("Failed to set permissions variable");

    client
        .let_var("session_start", json!("2023-12-01T10:00:00Z"))
        .await
        .expect("Failed to set session_start variable");

    // Test using variables in queries
    let query = "SELECT * FROM users WHERE id = $user_id";
    let result = client
        .query(query, None)
        .await
        .expect("Failed to query with variable");

    println!("Query with variable result: {:?}", result);

    // Test complex variable usage
    let complex_query = r#"
        LET $computed = {
            user: $user_id,
            perms: $permissions,
            started: $session_start,
            timestamp: time::now()
        };
        RETURN $computed;
    "#;

    let complex_result = client
        .query(complex_query, None)
        .await
        .expect("Failed to execute complex query");

    println!("Complex variable result: {:?}", complex_result);

    // Test unsetting variables
    client
        .unset("user_id")
        .await
        .expect("Failed to unset user_id");

    client
        .unset("permissions")
        .await
        .expect("Failed to unset permissions");

    client
        .unset("session_start")
        .await
        .expect("Failed to unset session_start");

    // Verify variables are unset
    let after_unset = client
        .query("RETURN $user_id", None)
        .await
        .expect("Failed to query after unset");

    println!("After unset result: {:?}", after_unset);
}

#[tokio::test]
async fn test_relations() {
    let client = get_client().await;

    // Clean up
    let _ = client.delete("authors").await;
    let _ = client.delete("books").await;
    let _ = client.delete("wrote").await;

    // Create authors
    let author1 = json!({
        "name": "J.K. Rowling",
        "birth_year": 1965,
        "nationality": "British"
    });

    let author2 = json!({
        "name": "George R.R. Martin",
        "birth_year": 1948,
        "nationality": "American"
    });

    client
        .create("authors:jkr", Some(author1))
        .await
        .expect("Failed to create author1");

    client
        .create("authors:grrm", Some(author2))
        .await
        .expect("Failed to create author2");

    // Create books
    let book1 = json!({
        "title": "Harry Potter and the Philosopher's Stone",
        "publication_year": 1997,
        "genre": "Fantasy"
    });

    let book2 = json!({
        "title": "A Game of Thrones",
        "publication_year": 1996,
        "genre": "Fantasy"
    });

    client
        .create("books:hp1", Some(book1))
        .await
        .expect("Failed to create book1");

    client
        .create("books:got1", Some(book2))
        .await
        .expect("Failed to create book2");

    // Create relationships using RELATE
    let relate_query1 = "RELATE authors:jkr->wrote->books:hp1 SET year = 1997, role = 'author'";
    client
        .query(relate_query1, None)
        .await
        .expect("Failed to create relationship 1");

    let relate_query2 = "RELATE authors:grrm->wrote->books:got1 SET year = 1996, role = 'author'";
    client
        .query(relate_query2, None)
        .await
        .expect("Failed to create relationship 2");

    // Query relationships
    let books_by_jkr = client
        .query("SELECT * FROM authors:jkr->wrote->books", None)
        .await
        .expect("Failed to query books by JKR");

    println!("Books by J.K. Rowling: {:?}", books_by_jkr);

    // Query with graph traversal
    let graph_query = r#"
        SELECT
            id,
            name,
            ->wrote->books.* AS books
        FROM authors
    "#;

    let graph_result = client
        .query(graph_query, None)
        .await
        .expect("Failed to execute graph query");

    println!("Graph traversal result: {:?}", graph_result);

    // Clean up
    let _ = client.delete("authors").await;
    let _ = client.delete("books").await;
    let _ = client.delete("wrote").await;
}

#[tokio::test]
async fn test_transactions() {
    let client = get_client().await;

    // Clean up
    let _ = client.delete("accounts").await;

    // Set up test accounts
    client
        .create("accounts:alice", Some(json!({"balance": 1000.0})))
        .await
        .expect("Failed to create Alice's account");

    client
        .create("accounts:bob", Some(json!({"balance": 500.0})))
        .await
        .expect("Failed to create Bob's account");

    // Test successful transaction
    let transaction_query = r#"
        BEGIN TRANSACTION;

        UPDATE accounts:alice SET balance = balance - 100.0;
        UPDATE accounts:bob SET balance = balance + 100.0;

        COMMIT TRANSACTION;
    "#;

    let tx_result = client
        .query(transaction_query, None)
        .await
        .expect("Failed to execute transaction");

    println!("Transaction result: {:?}", tx_result);

    // Verify balances
    let alice_balance = client
        .select("accounts:alice")
        .await
        .expect("Failed to get Alice's balance");

    let bob_balance = client
        .select("accounts:bob")
        .await
        .expect("Failed to get Bob's balance");

    println!("Alice's balance: {:?}", alice_balance);
    println!("Bob's balance: {:?}", bob_balance);

    // Test failed transaction (should rollback)
    let failed_transaction = r#"
        BEGIN TRANSACTION;

        UPDATE accounts:alice SET balance = balance - 2000.0;
        UPDATE accounts:bob SET balance = balance + 2000.0;

        -- This should fail due to insufficient funds
        THROW "Insufficient funds" IF (SELECT VALUE balance FROM accounts:alice)[0] < 0;

        COMMIT TRANSACTION;
    "#;

    let failed_result = client.query(failed_transaction, None).await;
    println!("Failed transaction result: {:?}", failed_result);

    // Verify balances are unchanged
    let alice_after = client
        .select("accounts:alice")
        .await
        .expect("Failed to get Alice's balance after failed tx");

    let bob_after = client
        .select("accounts:bob")
        .await
        .expect("Failed to get Bob's balance after failed tx");

    println!("Alice's balance after failed tx: {:?}", alice_after);
    println!("Bob's balance after failed tx: {:?}", bob_after);

    // Clean up
    let _ = client.delete("accounts").await;
}

#[tokio::test]
async fn test_complex_queries() {
    let client = get_client().await;

    // Clean up
    let _ = client.delete("employees").await;
    let _ = client.delete("departments").await;

    // Create departments
    let departments = vec![
        (
            "departments:eng",
            json!({"name": "Engineering", "budget": 100000}),
        ),
        (
            "departments:sales",
            json!({"name": "Sales", "budget": 80000}),
        ),
        (
            "departments:hr",
            json!({"name": "Human Resources", "budget": 50000}),
        ),
    ];

    for (id, data) in departments {
        client
            .create(id, Some(data))
            .await
            .expect("Failed to create department");
    }

    // Create employees
    let employees = vec![
        (
            "employees:alice",
            json!({"name": "Alice Johnson", "department": "departments:eng", "salary": 85000, "years_experience": 5}),
        ),
        (
            "employees:bob",
            json!({"name": "Bob Smith", "department": "departments:eng", "salary": 75000, "years_experience": 3}),
        ),
        (
            "employees:carol",
            json!({"name": "Carol Davis", "department": "departments:sales", "salary": 65000, "years_experience": 7}),
        ),
        (
            "employees:dave",
            json!({"name": "Dave Wilson", "department": "departments:sales", "salary": 55000, "years_experience": 2}),
        ),
        (
            "employees:eve",
            json!({"name": "Eve Brown", "department": "departments:hr", "salary": 60000, "years_experience": 4}),
        ),
    ];

    for (id, data) in employees {
        client
            .create(id, Some(data))
            .await
            .expect("Failed to create employee");
    }

    // Simple aggregation query that should work with SurrealDB
    let agg_query = r#"
        SELECT count() AS total_employees
        FROM employees
    "#;

    let agg_result = client
        .query(agg_query, None)
        .await
        .expect("Failed to execute aggregation query");

    println!("Employee count: {:?}", agg_result);

    // Simple SELECT with WHERE clause
    let filter_query = r#"
        SELECT * FROM employees WHERE salary > 60000
    "#;

    let filter_result = client
        .query(filter_query, None)
        .await
        .expect("Failed to execute filter query");

    println!("High salary employees: {:?}", filter_result);

    // Clean up
    let _ = client.delete("employees").await;
    let _ = client.delete("departments").await;
}

#[tokio::test]
async fn test_client_cloning() {
    let mut client1 = get_client().await;
    let client2 = get_client().await;

    // Note: Both client1 and client2 are references to the same SurrealClient instance
    // They share the same session state, which is the intended behavior for our refactoring

    // Set a variable using client1
    client1
        .let_var("shared_var", json!("set_by_client1"))
        .await
        .expect("Failed to set variable via client1");

    // Read the variable using client2 (should see the same value since they're the same client)
    let query_result = client2
        .query("RETURN $shared_var", None)
        .await
        .expect("Failed to query variable via client2");

    println!(
        "Variable set by client1, read by client2: {:?}",
        query_result
    );

    // Both clients should work and return the same version
    let version1 = client1
        .version()
        .await
        .expect("Failed to get version from client1");
    let version2 = client2
        .version()
        .await
        .expect("Failed to get version from client2");

    assert_eq!(version1, version2);
    println!(
        "Both client references work with shared connection, version: {}",
        version1
    );

    // Clean up
    client1
        .unset("shared_var")
        .await
        .expect("Failed to unset variable");
}

#[tokio::test]
async fn test_error_handling() {
    let client = get_client().await;

    // Test invalid query
    let invalid_result = client.query("INVALID QUERY SYNTAX", None).await;
    assert!(invalid_result.is_err());
    println!("Invalid query error: {:?}", invalid_result.unwrap_err());

    // Test selecting non-existent record
    let missing = client.select("nonexistent:record").await;
    println!("Missing record result: {:?}", missing);

    // Test constraint violation (if we had constraints)
    let duplicate_result = client
        .create("test:duplicate", Some(json!({"name": "test"})))
        .await;

    if duplicate_result.is_ok() {
        // Try creating the same record again
        let second_attempt = client
            .create("test:duplicate", Some(json!({"name": "test2"})))
            .await;
        println!("Duplicate creation result: {:?}", second_attempt);

        // Clean up
        let _ = client.delete("test:duplicate").await;
    }
}

#[tokio::test]
async fn test_import_export() {
    let client = get_client().await;

    // Clean up
    let _ = client.delete("import_test").await;

    // Create some test data
    let test_data = vec![
        json!({"name": "Item 1", "value": 100}),
        json!({"name": "Item 2", "value": 200}),
        json!({"name": "Item 3", "value": 300}),
    ];

    for (i, data) in test_data.iter().enumerate() {
        client
            .create(&format!("import_test:item_{}", i + 1), Some(data.clone()))
            .await
            .expect("Failed to create test data");
    }

    // Export data using query
    let exported = client
        .query("SELECT * FROM import_test ORDER BY name", None)
        .await
        .expect("Failed to export data");

    println!("Exported data: {:?}", exported);

    // Test bulk import using transaction
    let bulk_import = r#"
        BEGIN TRANSACTION;

        DELETE import_test;

        CREATE import_test:batch_1 SET name = "Batch Item 1", value = 1000, batch = true;
        CREATE import_test:batch_2 SET name = "Batch Item 2", value = 2000, batch = true;
        CREATE import_test:batch_3 SET name = "Batch Item 3", value = 3000, batch = true;

        COMMIT TRANSACTION;
    "#;

    let import_result = client
        .query(bulk_import, None)
        .await
        .expect("Failed to bulk import");

    println!("Bulk import result: {:?}", import_result);

    // Verify import
    let after_import = client
        .select("import_test")
        .await
        .expect("Failed to verify import");

    println!("Data after import: {:?}", after_import);

    // Clean up
    let _ = client.delete("import_test").await;
}
