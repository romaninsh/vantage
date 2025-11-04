use serde_json::json;
use vantage_table::mocks::*;
use vantage_table::prelude::*;

#[tokio::test]
async fn test_table_like_get_values() {
    // Create a mock table source with test data
    let data_source = MockTableSource::new().with_data(
        "users",
        vec![
            json!({"id": "1", "name": "Alice", "email": "alice@example.com"}),
            json!({"id": "2", "name": "Bob", "email": "bob@example.com"}),
        ],
    );

    // Create a table
    let table = Table::new("users", data_source)
        .with_column("id")
        .with_column("name")
        .with_column("email");

    // Convert to TableLike trait object (this is what UI grids would do)
    let table_like: Box<dyn TableLike> = Box::new(table);

    // Test get_values() method
    let values = table_like.get_values().await.unwrap();

    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["name"], "Alice");
    assert_eq!(values[1]["name"], "Bob");
    assert_eq!(values[0]["email"], "alice@example.com");
    assert_eq!(values[1]["email"], "bob@example.com");
}

#[tokio::test]
async fn test_table_like_get_values_empty() {
    // Create empty mock table source
    let data_source = MockTableSource::new().with_data("empty_table", vec![]);

    let table = Table::new("empty_table", data_source).with_column("id");

    let table_like: Box<dyn TableLike> = Box::new(table);

    let values = table_like.get_values().await.unwrap();
    assert_eq!(values.len(), 0);
}
