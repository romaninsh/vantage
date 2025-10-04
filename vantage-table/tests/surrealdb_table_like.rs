use serde_json::Value;
use surreal_client::error::Result;
use surreal_client::{Engine, SurrealClient};
use vantage_surrealdb::SurrealDB;
use vantage_table::prelude::*;

struct MockEngine;

#[async_trait::async_trait]
impl Engine for MockEngine {
    async fn send_message(&mut self, _method: &str, _params: Value) -> Result<Value> {
        Ok(serde_json::Value::Null)
    }
}

#[tokio::test]
async fn test_surrealdb_table_like() {
    // Create a mock SurrealDB instance
    let client = SurrealClient::new(
        Box::new(MockEngine),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    let surrealdb = SurrealDB::new(client);

    // Create a table with SurrealDB
    let table = Table::new("users", surrealdb)
        .with_column("id")
        .with_column("name")
        .with_column("email");

    // Convert to TableLike for dynamic dispatch
    let table_like: Box<dyn TableLike> = Box::new(table);

    // Test that we can get columns through TableLike
    let columns = table_like.columns();
    assert_eq!(columns.len(), 3);

    // Check column names
    assert_eq!(columns[0].name(), "id");
    assert_eq!(columns[1].name(), "name");
    assert_eq!(columns[2].name(), "email");

    // Check that aliases are None by default
    assert_eq!(columns[0].alias(), None);
    assert_eq!(columns[1].alias(), None);
    assert_eq!(columns[2].alias(), None);
}

#[tokio::test]
async fn test_mixed_datasource_tables_as_table_like() {
    // Create different types of data sources
    let mock_datasource = vantage_table::mocks::MockTableSource::new();
    let users_table = Table::new("users", mock_datasource)
        .with_column("id")
        .with_column("username");

    let client = SurrealClient::new(
        Box::new(MockEngine),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    let surrealdb = SurrealDB::new(client);
    let products_table = Table::new("products", surrealdb)
        .with_column("product_id")
        .with_column("name")
        .with_column("price");

    // Store both as TableLike - this is the key benefit!
    let tables: Vec<Box<dyn TableLike>> = vec![Box::new(users_table), Box::new(products_table)];

    // Process all tables uniformly regardless of their underlying datasource
    assert_eq!(tables.len(), 2);

    // Check first table (StaticDataSource)
    let users_columns = tables[0].columns();
    assert_eq!(users_columns.len(), 2);
    assert_eq!(users_columns[0].name(), "id");
    assert_eq!(users_columns[1].name(), "username");

    // Check second table (SurrealDB)
    let products_columns = tables[1].columns();
    assert_eq!(products_columns.len(), 3);
    assert_eq!(products_columns[0].name(), "product_id");
    assert_eq!(products_columns[1].name(), "name");
    assert_eq!(products_columns[2].name(), "price");
}

#[tokio::test]
async fn test_surreal_column_expressions() {
    let client = SurrealClient::new(
        Box::new(MockEngine),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    let surrealdb = SurrealDB::new(client);

    let table = Table::new("test_table", surrealdb)
        .with_column("name")
        .with_column("SELECT"); // Reserved keyword to test escaping

    let table_like: Box<dyn TableLike> = Box::new(table);
    let columns = table_like.columns();

    // Test that expressions are properly formed
    let name_expr = columns[0].expr();
    let select_expr = columns[1].expr();

    assert_eq!(name_expr.preview(), "name");
    assert_eq!(select_expr.preview(), "⟨SELECT⟩"); // Should be escaped
}
