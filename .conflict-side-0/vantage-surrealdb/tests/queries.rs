use serde::{Deserialize, Serialize};
use serde_json::json;
use vantage_expressions::AssociatedQueryable;
use vantage_surrealdb::{mocks::SurrealMockBuilder, prelude::*};
use vantage_table::prelude::*;

#[tokio::test]
async fn test_associated_query_get_raw() {
    let mock_data = json!([
        {"from": "foo@bar", "return": "test_value"},
        {"from": "baz@qux", "return": "other_value"}
    ]);

    let db = SurrealMockBuilder::new()
        .with_query_response("SELECT ⟨from⟩, ⟨return⟩ FROM message", mock_data)
        .build();

    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Message {
        pub from: String,
        pub r#return: String,
    }

    let messages = Table::new("message", db)
        .with_column("from")
        .with_column("return")
        .into_entity::<Message>();

    // Get raw data
    let query = messages.select_surreal();
    let client_list = query.get().await.unwrap();
    println!("Raw data: {:?}", client_list);
    assert_eq!(client_list[0].from, "foo@bar");
}

#[tokio::test]
async fn test_get_with_ids() {
    let mock_data = json!([
        {"id": "message:123", "from": "alice@example.com", "return": "hello"},
        {"id": "message:456", "from": "bob@example.com", "return": "world"}
    ]);

    let db = SurrealMockBuilder::new()
        .with_query_response("SELECT ⟨from⟩, ⟨return⟩, id FROM message", mock_data)
        .build();

    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Message {
        pub from: String,
        pub r#return: String,
    }

    let messages = Table::new("message", db)
        .with_column("from")
        .with_column("return")
        .into_entity::<Message>();

    // Test get_with_ids
    let results = messages.get_with_ids().await.unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "message:123");
    assert_eq!(results[0].1.from, "alice@example.com");
    assert_eq!(results[0].1.r#return, "hello");

    assert_eq!(results[1].0, "message:456");
    assert_eq!(results[1].1.from, "bob@example.com");
    assert_eq!(results[1].1.r#return, "world");
}

#[tokio::test]
async fn test_map() {
    let mock_data = json!([
        {"id": "message:123", "from": "alice@example.com", "return": "hello"},
        {"id": "message:456", "from": "bob@example.com", "return": "world"}
    ]);

    let db = SurrealMockBuilder::new()
        .with_query_response("SELECT ⟨from⟩, ⟨return⟩, id FROM message", mock_data)
        .with_exact_response(
            "merge",
            json!(["message:123", {"return": "HELLO"}]),
            json!({"id": "message:123", "from": "alice@example.com", "return": "HELLO"}),
        )
        .with_exact_response(
            "merge",
            json!(["message:456", {"return": "WORLD"}]),
            json!({"id": "message:456", "from": "bob@example.com", "return": "WORLD"}),
        )
        .build();

    #[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
    struct Message {
        pub from: String,
        pub r#return: String,
    }

    let messages = Table::new("message", db)
        .with_column("from")
        .with_column("return")
        .into_entity::<Message>();

    // Test map - transform all return values to uppercase
    let result = messages
        .map(|mut msg| {
            msg.r#return = msg.r#return.to_uppercase();
            msg
        })
        .await;

    // Should succeed now that we have proper mock responses
    assert!(result.is_ok());
}
