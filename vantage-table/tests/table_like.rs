use vantage_table::mocks::MockTableSource;
use vantage_table::prelude::*;

#[test]
fn test_table_like_dynamic_dispatch() {
    let datasource = MockTableSource::new();
    let table = Table::new("users", datasource)
        .with_column("id")
        .with_column("name")
        .with_column("email");

    // Convert to Box<dyn TableLike> for dynamic dispatch
    let table_like: Box<dyn TableLike> = Box::new(table);

    // Now we can use it without generics
    let columns = table_like.columns();

    assert_eq!(columns.len(), 3);
    assert!(columns.contains_key("id"));
    assert!(columns.contains_key("name"));
    assert!(columns.contains_key("email"));
    assert_eq!(columns["email"].alias(), None);
}

#[test]
fn test_multiple_tables_as_table_like() {
    // Create different types of tables
    let datasource1 = MockTableSource::new();
    let users_table = Table::new("users", datasource1)
        .with_column("id")
        .with_column("name");

    let datasource2 = MockTableSource::new();
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
        for (_key, column) in columns.iter() {
            assert!(!column.name().is_empty());
        }
    }
}
