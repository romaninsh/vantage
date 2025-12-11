use vantage_table::prelude::*;
use vantage_types::EmptyEntity;

#[test]
fn test_table_like_dynamic_dispatch() {
    let datasource = MockTableSource::new();
    let table = Table::<MockTableSource, EmptyEntity>::new("users", datasource)
        .with_column_of::<String>("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("email");

    // Convert to AnyTable for type erasure
    let any_table = AnyTable::new(table);

    // Test basic table operations
    assert_eq!(any_table.table_name(), "users");
    assert_eq!(any_table.table_alias(), "users");
}

#[test]
fn test_multiple_tables_as_anytable() {
    // Create different types of tables
    let datasource1 = MockTableSource::new();
    let users_table = Table::<MockTableSource, EmptyEntity>::new("users", datasource1)
        .with_column_of::<String>("id")
        .with_column_of::<String>("name");

    let datasource2 = MockTableSource::new();
    let orders_table = Table::<MockTableSource, EmptyEntity>::new("orders", datasource2)
        .with_column_of::<String>("order_id")
        .with_column_of::<i64>("amount");

    // Store them all as AnyTable for uniform handling
    let tables: Vec<AnyTable> = vec![AnyTable::new(users_table), AnyTable::new(orders_table)];

    // Process all tables uniformly
    for table in &tables {
        assert!(!table.table_name().is_empty());

        // Test that we can downcast back to concrete type
        assert!(table.is_type::<MockTableSource, EmptyEntity>());
    }
}

#[tokio::test]
async fn test_anytable_downcasting() {
    let datasource = MockTableSource::new();
    let table = Table::<MockTableSource, EmptyEntity>::new("products", datasource)
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");

    // Convert to AnyTable
    let any_table = AnyTable::new(table);

    // Test type checking
    assert!(any_table.is_type::<MockTableSource, EmptyEntity>());

    // Test downcasting back to concrete type
    let concrete_table = any_table
        .clone()
        .downcast::<MockTableSource, EmptyEntity>()
        .unwrap();
    assert_eq!(concrete_table.table_name(), "products");

    // Test that wrong type fails
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    struct DifferentEntity {
        id: String,
    }

    let datasource2 = MockTableSource::new();
    let table2 = Table::<MockTableSource, DifferentEntity>::new("other", datasource2);
    let any_table2 = AnyTable::new(table2);

    // Should not match EmptyEntity
    assert!(!any_table2.is_type::<MockTableSource, EmptyEntity>());
    assert!(!any_table.is_type::<MockTableSource, DifferentEntity>());
}

#[tokio::test]
async fn test_anytable_value_operations() {
    let datasource = MockTableSource::new();
    let table = Table::<MockTableSource, EmptyEntity>::new("items", datasource);
    let any_table = AnyTable::new(table);

    // Test that we can call async methods
    let values = any_table.get_values().await;
    assert!(values.is_ok());

    // Test JSON value insertion
    let item_data = serde_json::json!({
        "name": "Test Item",
        "price": 99,
        "available": true
    });

    let result = any_table.insert_value_from_json("item1", item_data).await;
    assert!(result.is_ok());

    // Test retrieval
    let retrieved = any_table.get_value_as_json("item1").await;
    assert!(retrieved.is_ok());
}
