use serde_json::Value;

use vantage_surrealdb::SurrealDB;
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
async fn test_get_rows() {
    let mock_data = serde_json::json!([
        {"name": "John Doe", "email": "john@example.com"},
        {"name": "Jane Smith", "email": "jane@example.com"}
    ]);
    let db = setup_test_db_with_data(mock_data).await;
    let mut table = Table::new("users", db);

    // Add columns to the table
    table.add_column(Column::new("name"));
    table.add_column(Column::new("email"));

    // Test the table structure
    assert_eq!(table.table_name(), "users");
    assert_eq!(table.columns().len(), 2);
    assert!(table.columns().contains_key("name"));
    assert!(table.columns().contains_key("email"));

    // Test actual execution using SurrealDB's get method
    let result = table.get().await;

    // SurrealDB returns an array of values
    let rows = result.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let row = rows[0].as_object().unwrap();
    assert!(row.contains_key("name"));
    assert!(row.contains_key("email"));
}
