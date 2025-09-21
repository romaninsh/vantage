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
    impl Entity for Message {}

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
