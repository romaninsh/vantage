use surreal_client::SurrealClient;
use vantage_config::VantageConfig;
use vantage_surrealdb::{mocks::MockSurrealEngine, SurrealDB};
use vantage_table::prelude::{AnyTable, ColumnCollectionExt, ColumnFlag, TableLike};

#[test]
fn test_hidden_column_flag() {
    let yaml = r#"
entities:
  user:
    table: user
    id_column: id
    columns:
      - name: id
        type: string
      - name: name
        type: string
      - name: email
        type: string
      - name: is_deleted
        type: bool
        default: false
        hidden: true
"#;

    let config: VantageConfig = serde_yaml::from_str(yaml).expect("Failed to parse YAML");

    // Create a mock SurrealDB instance
    let client = SurrealClient::new(Box::new(MockSurrealEngine::new()), None, None);
    let db = SurrealDB::new(client);

    // Get table and convert to AnyTable
    let table = config
        .get_table("user", db)
        .expect("Failed to get user table");

    let any_table = AnyTable::new(table);

    // Get columns through AnyTable
    let columns = any_table.columns();

    // Check that is_deleted column has Hidden flag
    let is_deleted = columns.get("is_deleted").unwrap();
    assert!(
        is_deleted.flags().contains(&ColumnFlag::Hidden),
        "is_deleted column should have Hidden flag"
    );

    // Check that other columns don't have Hidden flag
    let name = columns.get("name").expect("name column not found");
    assert!(
        !name.flags().contains(&ColumnFlag::Hidden),
        "name column should not have Hidden flag"
    );

    let email = columns.get("email").expect("email column not found");
    assert!(
        !email.flags().contains(&ColumnFlag::Hidden),
        "email column should not have Hidden flag"
    );

    // Test the new .exclude() API - get only visible columns
    let visible_columns = any_table.columns().exclude(ColumnFlag::Hidden);
    assert_eq!(visible_columns.len(), 3, "Should have 3 visible columns");
    assert!(visible_columns.contains_key("id"));
    assert!(visible_columns.contains_key("name"));
    assert!(visible_columns.contains_key("email"));
    assert!(!visible_columns.contains_key("is_deleted"));

    // Test .only() API - get only hidden columns
    let hidden_columns = any_table.columns().only(ColumnFlag::Hidden);
    assert_eq!(hidden_columns.len(), 1, "Should have 1 hidden column");
    assert!(hidden_columns.contains_key("is_deleted"));
}
