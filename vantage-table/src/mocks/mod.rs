pub mod mock_column;
pub mod mock_table_source;
pub mod mock_type_system;

pub use mock_column::MockColumn;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;
    use mock_table_source::MockTableSource;
    use rust_decimal::Decimal;
    use serde::{Deserialize, Serialize};
    use vantage_dataset::ReadableValueSet;

    #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
    pub struct User {
        pub name: String,
        pub email: String,
        pub age: i64,
        pub balance: Decimal,
        pub is_active: bool,
    }

    #[tokio::test]
    async fn test_mock_table_with_data() {
        use serde_json::json;

        // Create mock data
        let test_data = vec![
            json!({
                "id": "user1",
                "name": "John Doe",
                "email": "john@example.com",
                "age": 30,
                "balance": {"decimal": "100.50"},
                "is_active": true
            }),
            json!({
                "id": "user2",
                "name": "Jane Smith",
                "email": "jane@example.com",
                "age": 25,
                "balance": {"decimal": "250.75"},
                "is_active": false
            }),
        ];

        // Set up mock query source for expression support
        use vantage_expressions::mocks::mock_builder;
        let mock_query_source = mock_builder::new();

        let mock_ds = MockTableSource::new()
            .with_data("users", test_data)
            .await
            .with_query_source(mock_query_source);

        let table: Table<MockTableSource, User> = Table::new("users", mock_ds)
            .with_column(MockColumn::<String>::new("id"))
            .with_column(MockColumn::<String>::new("name"))
            .with_column(MockColumn::<String>::new("email"))
            .with_column(MockColumn::<i64>::new("age"))
            .with_column(MockColumn::<Decimal>::new("balance"))
            .with_column(MockColumn::<bool>::new("is_active"));

        // Test data loading through list_values()
        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 2);

        // Verify data content
        assert!(values.contains_key("user1"));
        assert!(values.contains_key("user2"));

        // Test specific value retrieval
        let user1_record = &values["user1"];
        assert_eq!(user1_record["name"], json!("John Doe"));
        assert_eq!(user1_record["email"], json!("john@example.com"));
        assert_eq!(user1_record["age"], json!(30));
        assert_eq!(user1_record["is_active"], json!(true));

        let user2_record = &values["user2"];
        assert_eq!(user2_record["name"], json!("Jane Smith"));
        assert_eq!(user2_record["is_active"], json!(false));

        // Test count
        let count = table.get_count().await.unwrap();
        assert_eq!(count, 2);

        // Test expression-based count
        // TODO: Fix type conversion from serde_json::Value to usize in expression system
        // let count_expr = table.get_expr_count().get().await.unwrap();
        // assert_eq!(count_expr, 2);
    }
}
