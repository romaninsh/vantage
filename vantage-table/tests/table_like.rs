use vantage_expressions::mocks::StaticDataSource;
use vantage_table::prelude::*;

#[test]
fn test_table_like_dynamic_dispatch() {
    let datasource = StaticDataSource::new(serde_json::json!([]));
    let table = Table::new("users", datasource)
        .with_column("id")
        .with_column("name")
        .with_column("email");

    // Convert to Box<dyn TableLike> for dynamic dispatch
    let table_like: Box<dyn TableLike> = Box::new(table);

    // Now we can use it without generics
    let columns = table_like.columns();

    assert_eq!(columns.len(), 3);
    assert_eq!(columns[0].name(), "id");
    assert_eq!(columns[1].name(), "name");
    assert_eq!(columns[2].name(), "email");
    assert_eq!(columns[2].alias(), None);
}

#[test]
fn test_multiple_tables_as_table_like() {
    // Create different types of tables
    let datasource1 = StaticDataSource::new(serde_json::json!([]));
    let users_table = Table::new("users", datasource1)
        .with_column("id")
        .with_column("name");

    let datasource2 = StaticDataSource::new(serde_json::json!([]));
    let orders_table = Table::new("orders", datasource2)
        .with_column("order_id")
        .with_column("amount");

    // Store them all as TableLike for uniform handling
    let tables: Vec<Box<dyn TableLike>> = vec![Box::new(users_table), Box::new(orders_table)];

    // Process all tables uniformly
    for table in &tables {
        let columns = table.columns();
        assert!(!columns.is_empty());

        // All columns should have names
        for column in columns {
            assert!(!column.name().is_empty());
        }
    }
}
