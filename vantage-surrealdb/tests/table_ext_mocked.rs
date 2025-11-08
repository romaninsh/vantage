use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vantage_expressions::AssociatedQueryable;
use vantage_surrealdb::{mocks::SurrealMockBuilder, prelude::*};
use vantage_table::Table;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Client {
    pub name: String,
    pub email: String,
}

impl Client {
    pub fn table(db: SurrealDB) -> Table<SurrealDB, Client> {
        Table::new("client", db)
            .with_column("name")
            .with_column("email")
            .into_entity()
    }
}

#[tokio::test]
async fn test_associated_query_get_raw() -> Result<(), Box<dyn std::error::Error>> {
    let mock_data = json!([
        {"name": "John Doe", "email": "john@example.com"},
        {"name": "Jane Smith", "email": "jane@example.com"}
    ]);

    let db = SurrealMockBuilder::new()
        .with_query_response("SELECT name, email FROM client", mock_data)
        .build();

    // Get raw data
    let query = Client::table(db).select_surreal();
    let client_list = query.get().await?;
    println!("Raw data: {:?}", client_list);
    assert_eq!(client_list[0].name, "John Doe");

    Ok(())
}

#[tokio::test]
async fn test_associated_query_get_entity_as() -> Result<(), Box<dyn std::error::Error>> {
    let mock_data = json!({"name": "John Doe", "email": "john@example.com"});

    let db = SurrealMockBuilder::new()
        .with_query_response("SELECT name, email FROM ONLY client", mock_data)
        .build();

    // Get as arbitrary type (Value instead of Client)
    let query = Client::table(db).select_surreal_first();
    let row: Value = query.get_as_value().await?;
    println!("Clients: {:?}", row);
    assert_eq!(row["name"], "John Doe");

    Ok(())
}

#[tokio::test]
async fn test_associated_query_single_row() -> Result<(), Box<dyn std::error::Error>> {
    let mock_data = json!({"name": "John Doe", "email": "john@example.com"});

    let db = SurrealMockBuilder::new()
        .with_query_response("SELECT name, email FROM ONLY client", mock_data)
        .build();

    // Single row
    let query = Client::table(db).select_surreal_first();
    let client = query.get().await?;
    println!("Client name: {}", client.name);
    assert_eq!(client.name, "John Doe");

    Ok(())
}

#[tokio::test]
async fn test_associated_query_column() -> Result<(), Box<dyn std::error::Error>> {
    let mock_data = json!(["john@example.com", "jane@example.com"]);

    let db = SurrealMockBuilder::new()
        .with_query_response("SELECT VALUE email FROM client", mock_data)
        .build();

    // Single column
    let query = Client::table(db).select_surreal_column("email")?;
    let emails = query.get().await?;
    println!("Emails: {:?}", emails);
    assert_eq!(
        emails,
        vec![
            Value::String("john@example.com".to_string()),
            Value::String("jane@example.com".to_string())
        ]
    );

    Ok(())
}

#[tokio::test]
async fn test_mock_builder_exact_matching() -> Result<(), Box<dyn std::error::Error>> {
    // Test exact query matching
    let db = SurrealMockBuilder::new()
        .with_query_response(
            "SELECT name, email FROM client",
            json!([{"name": "Active User", "email": "active@example.com"}]),
        )
        .with_exact_response("count", json!({"table": "client"}), json!(42))
        .build();

    // Test that exact matching works
    let query = Client::table(db).select_surreal();
    let users = query.get().await?;
    assert_eq!(users[0].name, "Active User");

    Ok(())
}

#[tokio::test]
async fn test_method_specific_responses() -> Result<(), Box<dyn std::error::Error>> {
    let _db = SurrealMockBuilder::new()
        .with_method_response("query", json!([{"result": "from_query"}]))
        .with_method_response("select", json!([{"result": "from_select"}]))
        .with_method_response("create", json!({"id": "new_record", "result": "created"}))
        .with_method_response("delete", json!({"deleted": 1}))
        .build();

    // Different methods would return different responses
    // This demonstrates the flexibility of the mock system

    Ok(())
}

#[tokio::test]
async fn test_exact_matching_demonstration() -> Result<(), Box<dyn std::error::Error>> {
    // This test demonstrates the exact matching behavior
    let db = SurrealMockBuilder::new()
        .with_debug(true)
        .with_query_response(
            "SELECT name, email FROM client",
            json!([
                {"name": "John Doe", "email": "john@example.com"}
            ]),
        )
        .build();

    // This will work because it matches exactly
    let table = Table::new("client", db)
        .with_column("name")
        .with_column("email")
        .into_entity::<Client>();

    let users = table.select_surreal().get().await?;
    assert_eq!(users[0].name, "John Doe");

    Ok(())
}
