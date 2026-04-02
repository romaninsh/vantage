use surreal_client::{SurrealClient, SurrealConnection};
use vantage_expressions::prelude::*;
use vantage_surrealdb::{surreal_expr, surrealdb::SurrealDB};

const DB_URL: &str = "cbor://localhost:8000/rpc";
const ROOT_USER: &str = "root";
const ROOT_PASS: &str = "root";
const TEST_NAMESPACE: &str = "bakery";
const TEST_DATABASE: &str = "types";

async fn get_client() -> SurrealClient {
    SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root(ROOT_USER, ROOT_PASS)
        .with_debug(true)
        .connect()
        .await
        .expect("Failed to connect to SurrealDB")
}

async fn get_surrealdb() -> SurrealDB {
    let client = get_client().await;
    SurrealDB::new(client)
}

#[tokio::test]
async fn test_return_statement() {
    let db = get_surrealdb().await;

    let test_number = 42;

    let return_expr = surreal_expr!("RETURN {}", test_number);
    let result = db
        .execute(&return_expr)
        .await
        .expect("Failed to execute return query");

    use ciborium::Value;
    let Value::Integer(_returned_number) = result.value() else {
        panic!("Expected returned integer value")
    };
}

#[tokio::test]
async fn test_select_patterns() {
    let db = get_surrealdb().await;
    use ciborium::Value;

    // Create table with multiple rows
    let create_expr1 = surreal_expr!("CREATE select_patterns_table:1 SET num = {}", 123);
    let create_expr2 = surreal_expr!("CREATE select_patterns_table:2 SET num = {}", 456);
    let _create_result1 = db
        .execute(&create_expr1)
        .await
        .expect("Failed to create test record 1");
    let _create_result2 = db
        .execute(&create_expr2)
        .await
        .expect("Failed to create test record 2");

    println!("=== Test 1: SELECT columns (returns array of objects) ===");
    let select1_expr = surreal_expr!("SELECT num FROM select_patterns_table");
    let result1 = db
        .execute(&select1_expr)
        .await
        .expect("Failed to execute select1 query");
    println!("Result: {:?}", result1.value());

    println!("\n=== Test 2: SELECT VALUE (returns array of values) ===");
    let select2_expr = surreal_expr!("SELECT VALUE num FROM select_patterns_table");
    let result2 = db
        .execute(&select2_expr)
        .await
        .expect("Failed to execute select2 query");
    println!("Result: {:?}", result2.value());

    println!("\n=== Test 3: SELECT * (returns array of full records) ===");
    let select3_expr = surreal_expr!("SELECT * FROM select_patterns_table");
    let result3 = db
        .execute(&select3_expr)
        .await
        .expect("Failed to execute select3 query");
    println!("Result: {:?}", result3.value());

    println!("\n=== Test 4: SELECT ONLY (returns single record directly) ===");
    let select4_expr = surreal_expr!("SELECT * FROM ONLY select_patterns_table:1");
    let result4 = db
        .execute(&select4_expr)
        .await
        .expect("Failed to execute select4 query");
    println!("Result: {:?}", result4.value());

    // Verify ONLY returns single object (not array)
    let Value::Map(_single_record) = result4.value() else {
        panic!("ONLY should return single Map, not Array")
    };
    println!("✅ ONLY correctly returns single record directly");

    // Cleanup
    let cleanup_expr = surreal_expr!("DELETE select_patterns_table");
    let _ = db.execute(&cleanup_expr).await;
}

#[tokio::test]
async fn test_failing_expression() {
    let db = get_surrealdb().await;

    // Create an expression with invalid SQL syntax that will cause a parse error
    let failing_expr = surreal_expr!("SELECT =========");
    let result = db.execute(&failing_expr).await;

    // Verify it returns an error
    assert!(result.is_err(), "Expected query to fail but it succeeded");

    let error = result.unwrap_err();
    let error_string = error.to_string();

    // Verify error contains the specific "Parse error" message that can only come from database
    assert!(
        error_string.contains("Parse error"),
        "Error should contain 'Parse error' from database, got: {}",
        error_string
    );

    println!(
        "✅ Correctly caught parse error from database: {}",
        error_string
    );
}
