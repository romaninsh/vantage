//! Test async example for mockbuilder as referenced in expression.rs documentation

use serde_json::json;
use vantage_expressions::{
    expr, mocks::mockbuilder, traits::datasource::QuerySource, traits::expressive::DeferredFn,
};

// API call that fetches user IDs asynchronously
async fn get_user_ids() -> vantage_core::Result<serde_json::Value> {
    // Simulate API call - fetch from external service
    Ok(json!([1, 2, 3, 4, 5]))
}

#[tokio::test]
async fn test_async_example_from_expression_docs() {
    // Set up mock to handle the constructed query after flattening
    // When flattened, the deferred function result replaces the placeholder
    let mock = mockbuilder::new().with_flattening().on_exact_select(
        "SELECT * FROM orders WHERE user_id = ANY([1,2,3,4,5])",
        json!([
            {"id": 1, "user_id": 1, "amount": 99.99},
            {"id": 2, "user_id": 3, "amount": 149.50}
        ]),
    );

    // Build query synchronously - no async needed here!
    let query = expr!("SELECT * FROM orders WHERE user_id = ANY({})", {
        DeferredFn::from_fn(get_user_ids)
    });

    // Verify the query preview shows deferred placeholder
    assert_eq!(
        query.preview(),
        "SELECT * FROM orders WHERE user_id = ANY(**deferred())"
    );

    // Execute the query - API call happens automatically during execution
    let orders = mock.execute(&query).await.unwrap();

    // Verify we got the expected results
    let orders_array = orders.as_array().unwrap();
    assert_eq!(orders_array.len(), 2);
    assert_eq!(orders_array[0]["amount"], 99.99);
    assert_eq!(orders_array[1]["amount"], 149.50);
}

#[tokio::test]
async fn test_multiple_patterns_with_flattening() {
    let mock = mockbuilder::new()
        .with_flattening()
        .on_exact_select("SELECT COUNT(*) FROM users WHERE active = true", json!(42))
        .on_exact_select(
            "SELECT * FROM products WHERE category = \"electronics\"",
            json!([
                {"id": "prod1", "name": "Laptop", "price": 999.99}
            ]),
        );

    // Test first pattern
    let user_count_query = expr!("SELECT COUNT(*) FROM users WHERE active = {}", true);
    let count_result = mock.execute(&user_count_query).await.unwrap();
    assert_eq!(count_result, json!(42));

    // Test second pattern
    let products_query = expr!("SELECT * FROM products WHERE category = {}", "electronics");
    let products_result = mock.execute(&products_query).await.unwrap();
    assert_eq!(products_result.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_nested_expression_flattening() {
    let mock = mockbuilder::new()
        .with_flattening()
        .on_exact_select(
            "SELECT * FROM orders WHERE user_id IN (SELECT id FROM users WHERE department = \"engineering\")",
            json!([
                {"id": 100, "user_id": 1, "total": 250.00}
            ])
        );

    // Build nested query structure - with flattening enabled, this becomes one flat expression
    let user_subquery = expr!("SELECT id FROM users WHERE department = {}", "engineering");
    let main_query = expr!(
        "SELECT * FROM orders WHERE user_id IN ({})",
        (user_subquery)
    );

    let result = mock.execute(&main_query).await.unwrap();
    let orders = result.as_array().unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["id"], 100);
}

#[tokio::test]
async fn test_error_handling_no_match() {
    let mock = mockbuilder::new().on_exact_select("SELECT * FROM users", json!([]));

    let query = expr!("SELECT * FROM products");
    let result = mock.execute(&query).await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("No matching pattern found"));
}
