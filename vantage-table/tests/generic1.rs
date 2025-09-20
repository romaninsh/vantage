use serde::{Deserialize, Serialize};
use serde_json::Value;

use vantage_table::{Entity, Table};

fn setup_test_datasource_with_data(
    mock_data: Value,
) -> vantage_expressions::mocks::StaticDataSource {
    use vantage_expressions::mocks::StaticDataSource;

    StaticDataSource::new(mock_data)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct User {
    pub name: String,
    pub email: String,
}

impl Entity for User {}

#[tokio::test]
async fn test_get_rows() {
    let mock_data = serde_json::json!([
        {"name": "John Doe", "email": "john@example.com"},
        {"name": "Jane Smith", "email": "jane@example.com"}
    ]);
    let datasource = setup_test_datasource_with_data(mock_data);
    let table = Table::new("users", datasource)
        .with_column("name")
        .with_column("email");

    // Test the table structure
    assert_eq!(table.table_name(), "users");
    assert_eq!(table.columns().len(), 2);
    assert!(table.columns().contains_key("name"));
    assert!(table.columns().contains_key("email"));

    // Test actual table execution with StaticDataSource
    let result = table.get_values().await.unwrap();

    // StaticDataSource returns an array of values
    assert_eq!(result.len(), 2);
    let row = result[0].as_object().unwrap();
    assert!(row.contains_key("name"));
    assert!(row.contains_key("email"));
}

#[tokio::test]
async fn test_get_entities() {
    let mock_data = serde_json::json!([
        {"name": "John Doe", "email": "john@example.com"},
        {"name": "Jane Smith", "email": "jane@example.com"}
    ]);
    let datasource = setup_test_datasource_with_data(mock_data);
    let table = Table::new("users", datasource)
        .with_column("name")
        .with_column("email")
        .into_entity::<User>();

    // Test the get() method that returns typed entities
    let users = table.get().await.unwrap();

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].name, "John Doe");
    assert_eq!(users[0].email, "john@example.com");
    assert_eq!(users[1].name, "Jane Smith");
    assert_eq!(users[1].email, "jane@example.com");
}
