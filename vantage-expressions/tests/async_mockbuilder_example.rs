//! Test async example for mockbuilder as referenced in expression.rs documentation

use serde_json::json;
use vantage_expressions::{
    expr, mocks::mock_builder, traits::datasource::ExprDataSource, traits::expressive::DeferredFn,
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
    let mock = mock_builder::new().with_flattening().on_exact_select(
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
