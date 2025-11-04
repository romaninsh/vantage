use serde::{Deserialize, Serialize};
use vantage_table::prelude::*;

fn setup_test_datasource() -> vantage_table::mocks::MockTableSource {
    use vantage_table::mocks::MockTableSource;

    MockTableSource::new()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct User {
    pub name: String,
    pub email: String,
}

#[test]
fn test_table_structure() {
    let datasource = setup_test_datasource();
    let table = Table::new("users", datasource)
        .with_column("name")
        .with_column("email");

    // Test the table structure
    assert_eq!(table.table_name(), "users");
    assert_eq!(table.columns().len(), 2);
    assert!(table.columns().contains_key("name"));
    assert!(table.columns().contains_key("email"));
}

#[test]
fn test_entity_conversion() {
    let datasource = setup_test_datasource();
    let table = Table::new("users", datasource)
        .with_column("name")
        .with_column("email")
        .into_entity::<User>();

    // Test that entity conversion works
    assert_eq!(table.table_name(), "users");
    assert_eq!(table.columns().len(), 2);
}
