use serde_json::Value;
use vantage_expressions::expr;
use vantage_surrealdb::{SurrealDB, SurrealTableExt};
use vantage_table::{Column, Table};

async fn setup_test_db_with_data(mock_data: Value) -> SurrealDB {
    use surreal_client::{Engine, SurrealClient};

    struct MockEngine {
        data: Value,
    }

    impl MockEngine {
        fn new(data: Value) -> Self {
            Self { data }
        }
    }

    #[async_trait::async_trait]
    impl Engine for MockEngine {
        async fn send_message(
            &mut self,
            _method: &str,
            _params: Value,
        ) -> surreal_client::error::Result<Value> {
            Ok(self.data.clone())
        }
    }

    let client = SurrealClient::new(
        Box::new(MockEngine::new(mock_data)),
        Some("test".to_string()),
        Some("v1".to_string()),
    );

    SurrealDB::new(client)
}

#[tokio::test]
async fn test_select_surreal_methods() {
    let mock_data = serde_json::json!([
        {"name": "John Doe", "email": "john@example.com", "age": 30},
        {"name": "Jane Smith", "email": "jane@example.com", "age": 25}
    ]);
    let db = setup_test_db_with_data(mock_data).await;
    let mut table = Table::new("users", db);

    table.add_column(Column::new("name"));
    table.add_column(Column::new("email"));
    table.add_column(Column::new("age"));

    // Test select_surreal() - returns Rows
    let rows_select = table.select_surreal();
    assert_eq!(rows_select.preview(), "SELECT name, email, age FROM users");

    // Test select_surreal_first() - returns SingleRow with preserved column order
    let first_select = table.select_surreal_first();
    assert_eq!(
        first_select.preview(),
        "SELECT name, email, age FROM ONLY users"
    );

    // Test select_surreal_column() - returns List
    let column_select = table.select_surreal_column("name").unwrap();
    assert_eq!(column_select.preview(), "SELECT VALUE name FROM users");

    // Test select_surreal_single() - returns Single
    let single_select = table.select_surreal_single("email").unwrap();
    assert_eq!(
        single_select.preview(),
        "SELECT VALUE email FROM ONLY users"
    );
}

#[tokio::test]
async fn test_select_surreal_column_validation() {
    let mock_data = serde_json::json!([]);
    let db = setup_test_db_with_data(mock_data).await;
    let mut table = Table::new("users", db);

    table.add_column(Column::new("name"));
    table.add_column(Column::new("email"));

    // Test valid column
    let result = table.select_surreal_column("name");
    assert!(result.is_ok());

    // Test invalid column
    let result = table.select_surreal_column("nonexistent");
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Column 'nonexistent' not found in table"
    );
}

#[tokio::test]
async fn test_select_surreal_single_validation() {
    let mock_data = serde_json::json!([]);
    let db = setup_test_db_with_data(mock_data).await;
    let mut table = Table::new("users", db);

    table.add_column(Column::new("name"));
    table.add_column(Column::new("email"));

    // Test valid column
    let result = table.select_surreal_single("email");
    assert!(result.is_ok());

    // Test invalid column
    let result = table.select_surreal_single("invalid");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Column 'invalid' not found in table");
}

#[tokio::test]
async fn test_select_surreal_with_conditions() {
    let mock_data = serde_json::json!([
        {"name": "John Doe", "email": "john@example.com", "age": 30}
    ]);
    let db = setup_test_db_with_data(mock_data).await;
    let mut table = Table::new("users", db);

    table.add_column(Column::new("name"));
    table.add_column(Column::new("email"));
    table.add_condition(expr!("age > {}", 18));

    // Test that conditions are applied to all select methods
    let rows_select = table.select_surreal();
    assert!(rows_select.preview().contains("WHERE age > 18"));

    let first_select = table.select_surreal_first();
    assert!(first_select.preview().contains("WHERE age > 18"));

    let column_select = table.select_surreal_column("name").unwrap();
    assert!(column_select.preview().contains("WHERE age > 18"));

    let single_select = table.select_surreal_single("email").unwrap();
    assert!(single_select.preview().contains("WHERE age > 18"));
}

#[tokio::test]
async fn test_select_surreal_with_aliases() {
    let mock_data = serde_json::json!([]);
    let db = setup_test_db_with_data(mock_data).await;
    let mut table = Table::new("users", db);

    table.add_column(Column::new("name").with_alias("user_name"));
    table.add_column(Column::new("email").with_alias("user_email"));

    // Test that aliases are handled in regular select
    let rows_select = table.select_surreal();
    let query = rows_select.preview();
    assert!(query.contains("name AS user_name"));
    assert!(query.contains("email AS user_email"));

    // Test column select with aliased column
    let column_select = table.select_surreal_column("name").unwrap();
    assert_eq!(column_select.preview(), "SELECT VALUE name FROM users");
}
