use serde::{Deserialize, Serialize};
use vantage_table::prelude::*;

async fn setup_test_datasource() -> MockTableSource {
    MockTableSource::new().with_data("users", vec![]).await
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct User {
    pub name: String,
    pub email: String,
}

#[tokio::test]
async fn test_table_structure() {
    let datasource = setup_test_datasource().await;
    let table: Table<MockTableSource, User> =
        Table::<MockTableSource, User>::new("users", datasource)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email");

    // Test the table structure
    assert_eq!(table.table_name(), "users");
    assert_eq!(table.columns().len(), 2);
    assert!(table.columns().contains_key("name"));
    assert!(table.columns().contains_key("email"));
}

#[tokio::test]
async fn test_entity_conversion() {
    let datasource = setup_test_datasource().await;
    let table: Table<MockTableSource, User> =
        Table::<MockTableSource, User>::new("users", datasource)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .into_entity::<User>();

    // Test that entity conversion works
    assert_eq!(table.table_name(), "users");
    assert_eq!(table.columns().len(), 2);
}
