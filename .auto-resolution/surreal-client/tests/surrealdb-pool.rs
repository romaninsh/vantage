use futures_util::future::join_all;
use serde_json::json;

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
#[ignore]
async fn test_basic_connection() {
    let client = get_client().await;
    client
        .delete("users")
        .await
        .expect("Failed to delete users");

    println!("Starting concurrent operations with pool...");
    let start = tokio::time::Instant::now();
    let client = get_client().await;

    let futures = (0..10).map(|i| {
        let client = client.clone();
        async move {
            println!("Task {} starting", i);
            let task_start = tokio::time::Instant::now();

            let user_data = json!({
                "name": "John Doe",
                "email": "john@example.com",
                "age": 30,
                "active": true
            });

            client
                .query("SLEEP 1s", None)
                .await
                .expect("Failed to sleep");

            client
                .create("users", Some(user_data))
                .await
                .expect("Failed to create user");

            println!("Task {} completed in {:?}", i, task_start.elapsed());
        }
    });
    let _ = join_all(futures).await;

    let elapsed = start.elapsed();

    let data = client
        .select("users")
        .await
        .expect("Failed to select users");

    println!("Data returned: {:?}", data);

    // All threads must complete simultaneously
    if let Some(array) = data.as_array() {
        assert_eq!(array.len(), 10);
    } else {
        panic!("Expected array but got: {:?}", data);
    }
    println!("Total elapsed time: {:?}", elapsed);
    assert!(elapsed.as_secs() < 3, "Elapsed time: {:?}", elapsed);
}

#[tokio::test]
#[ignore]
async fn test_debug_pool() {
    let client = get_client().await;

    // Test basic ping
    println!("Testing ping...");
    match client.version().await {
        Ok(version) => println!("Version: {}", version),
        Err(e) => println!("Version error: {:?}", e),
    }

    // Test simple create
    println!("Testing create...");
    let user_data = json!({
        "name": "Test User",
        "email": "test@example.com"
    });

    match client.create("test_users", Some(user_data)).await {
        Ok(result) => println!("Create result: {:?}", result),
        Err(e) => println!("Create error: {:?}", e),
    }

    // Test select
    println!("Testing select...");
    match client.select("test_users").await {
        Ok(result) => println!("Select result: {:?}", result),
        Err(e) => println!("Select error: {:?}", e),
    }
}

#[tokio::test]
#[ignore]
async fn test_parallel_without_sleep() {
    let client = get_client().await;
    client
        .delete("speed_test")
        .await
        .expect("Failed to delete speed_test");

    println!("Starting concurrent operations WITHOUT sleep...");
    let start = tokio::time::Instant::now();

    let futures = (0..10).map(|i| {
        let client = client.clone();
        async move {
            println!("Task {} starting", i);
            let task_start = tokio::time::Instant::now();

            let user_data = json!({
                "name": format!("User {}", i),
                "number": i
            });

            client
                .create("speed_test", Some(user_data))
                .await
                .expect("Failed to create user");

            println!("Task {} completed in {:?}", i, task_start.elapsed());
        }
    });
    let _ = join_all(futures).await;

    let elapsed = start.elapsed();
    println!("Total elapsed time without sleep: {:?}", elapsed);

    let data = client
        .select("speed_test")
        .await
        .expect("Failed to select speed_test");

    if let Some(array) = data.as_array() {
        println!("Created {} records", array.len());
        assert_eq!(array.len(), 10);
    } else {
        panic!("Expected array but got: {:?}", data);
    }

    // Should be much faster without sleep
    assert!(elapsed.as_millis() < 5000, "Too slow: {:?}", elapsed);
}

#[tokio::test]
#[ignore]
async fn test_regular_ws_parallel() {
    // Use regular WebSocket (no pool) for comparison
    let client = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        // No .with_pool() call - uses regular WebSocket
        .connect()
        .await
        .expect("Failed to create regular WebSocket connection");

    client
        .delete("regular_test")
        .await
        .expect("Failed to delete regular_test");

    println!("Starting concurrent operations with REGULAR WebSocket...");
    let start = tokio::time::Instant::now();

    let futures = (0..10).map(|i| {
        let client = client.clone();
        async move {
            println!("Regular Task {} starting", i);
            let task_start = tokio::time::Instant::now();

            let user_data = json!({
                "name": format!("Regular User {}", i),
                "number": i
            });

            client
                .create("regular_test", Some(user_data))
                .await
                .expect("Failed to create user");

            println!("Regular Task {} completed in {:?}", i, task_start.elapsed());
        }
    });
    let _ = join_all(futures).await;

    let elapsed = start.elapsed();
    println!("Total elapsed time with regular WebSocket: {:?}", elapsed);

    let data = client
        .select("regular_test")
        .await
        .expect("Failed to select regular_test");

    if let Some(array) = data.as_array() {
        println!("Created {} records with regular WebSocket", array.len());
        assert_eq!(array.len(), 10);
    } else {
        panic!("Expected array but got: {:?}", data);
    }
}

#[tokio::test]
#[ignore]
async fn test_regular_ws_with_sleep() {
    // Use regular WebSocket (no pool) for comparison with SLEEP
    let client = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        // No .with_pool() call - uses regular WebSocket
        .connect()
        .await
        .expect("Failed to create regular WebSocket connection");

    client
        .delete("sleep_test")
        .await
        .expect("Failed to delete sleep_test");

    println!("Starting concurrent operations with REGULAR WebSocket + SLEEP...");
    let start = tokio::time::Instant::now();

    let futures = (0..10).map(|i| {
        let client = client.clone();
        async move {
            println!("Regular Sleep Task {} starting", i);
            let task_start = tokio::time::Instant::now();

            let user_data = json!({
                "name": format!("Sleep User {}", i),
                "number": i
            });

            client
                .query("SLEEP 1s", None)
                .await
                .expect("Failed to sleep");

            client
                .create("sleep_test", Some(user_data))
                .await
                .expect("Failed to create user");

            println!(
                "Regular Sleep Task {} completed in {:?}",
                i,
                task_start.elapsed()
            );
        }
    });
    let _ = join_all(futures).await;

    let elapsed = start.elapsed();
    println!(
        "Total elapsed time with regular WebSocket + SLEEP: {:?}",
        elapsed
    );

    let data = client
        .select("sleep_test")
        .await
        .expect("Failed to select sleep_test");

    if let Some(array) = data.as_array() {
        println!(
            "Created {} records with regular WebSocket + SLEEP",
            array.len()
        );
        assert_eq!(array.len(), 10);
    } else {
        panic!("Expected array but got: {:?}", data);
    }

    // Should be around 1 second if truly parallel
    println!("Expected ~1s, got {:?}", elapsed);
    assert!(
        elapsed.as_secs() < 3,
        "Too slow - not parallel: {:?}",
        elapsed
    );
}

#[tokio::test]
#[ignore]
async fn test_individual_connections_with_sleep() {
    println!("Starting concurrent operations with INDIVIDUAL connections + SLEEP...");
    let start = tokio::time::Instant::now();

    let futures = (0..10).map(|i| {
        async move {
            println!("Individual Task {} starting", i);
            let task_start = tokio::time::Instant::now();

            // Create individual connection per task
            let client = SurrealConnection::new()
                .url(DB_URL)
                .namespace(TEST_NAMESPACE)
                .database(TEST_DATABASE)
                .auth_root(ROOT_USER, ROOT_PASS)
                .connect()
                .await
                .expect("Failed to create individual connection");

            let user_data = json!({
                "name": format!("Individual User {}", i),
                "number": i
            });

            client
                .query("SLEEP 1s", None)
                .await
                .expect("Failed to sleep");

            client
                .create("individual_test", Some(user_data))
                .await
                .expect("Failed to create user");

            println!(
                "Individual Task {} completed in {:?}",
                i,
                task_start.elapsed()
            );
        }
    });
    let _ = join_all(futures).await;

    let elapsed = start.elapsed();
    println!(
        "Total elapsed time with individual connections + SLEEP: {:?}",
        elapsed
    );

    // Use any connection to check results
    let client = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await
        .expect("Failed to create check connection");

    let data = client
        .select("individual_test")
        .await
        .expect("Failed to select individual_test");

    if let Some(array) = data.as_array() {
        println!(
            "Created {} records with individual connections",
            array.len()
        );
        assert_eq!(array.len(), 10);
    } else {
        panic!("Expected array but got: {:?}", data);
    }

    // Should be around 1 second if truly parallel
    println!("Expected ~1s, got {:?}", elapsed);
    assert!(
        elapsed.as_secs() < 3,
        "Should be parallel with individual connections: {:?}",
        elapsed
    );
}

#[tokio::test]
#[ignore]
async fn test_single_connection_concurrent_requests() {
    println!("Testing single connection with concurrent requests...");

    // Create one connection manually
    let client = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .connect()
        .await
        .expect("Failed to create connection");

    client
        .delete("single_conn_test")
        .await
        .expect("Failed to delete single_conn_test");

    let start = tokio::time::Instant::now();

    // Use the SAME client instance for all concurrent requests
    let futures = (0..5).map(|i| {
        let client = &client; // Borrow, don't clone
        async move {
            println!("Single conn task {} starting", i);
            let task_start = tokio::time::Instant::now();

            let user_data = json!({
                "name": format!("Single User {}", i),
                "number": i
            });

            client
                .query("SLEEP 1s", None)
                .await
                .expect("Failed to sleep");

            client
                .create("single_conn_test", Some(user_data))
                .await
                .expect("Failed to create user");

            println!(
                "Single conn task {} completed in {:?}",
                i,
                task_start.elapsed()
            );
        }
    });

    let _ = join_all(futures).await;

    let elapsed = start.elapsed();
    println!("Single connection total time: {:?}", elapsed);

    let data = client
        .select("single_conn_test")
        .await
        .expect("Failed to select single_conn_test");

    if let Some(array) = data.as_array() {
        println!("Single connection created {} records", array.len());
        assert_eq!(array.len(), 5);
    }

    // If single connection can handle concurrent requests, this should be ~1s
    // If it serializes, this will be ~5s
    println!(
        "Single connection: Expected ~1s if concurrent, ~5s if serial. Got: {:?}",
        elapsed
    );
}
