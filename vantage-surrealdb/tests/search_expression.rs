use surreal_client::SurrealClient;
use vantage_surrealdb::SurrealDB;
use vantage_surrealdb::mocks::MockSurrealEngine;
use vantage_table::{Column, ColumnFlag, Table, TableSource};

#[test]
fn test_search_expression_with_searchable_columns() {
    // Create a mock SurrealDB instance
    let client = SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    let db = SurrealDB::new(client);

    // Create a table with searchable columns
    let table = Table::new("users", db.clone())
        .with_column(Column::new("id").with_flag(ColumnFlag::IdField))
        .with_column(
            Column::new("name").with_flags(&[ColumnFlag::TitleField, ColumnFlag::Searchable]),
        )
        .with_column(Column::new("email").with_flag(ColumnFlag::Searchable))
        .with_column(Column::new("password").with_flag(ColumnFlag::Hidden))
        .with_column(Column::new("age"));

    let any_table = vantage_table::prelude::AnyTable::new(table);

    // Generate search expression
    let search_expr = db.search_expression(&any_table, "john");

    let preview = search_expr.preview();

    // Should include name and email (searchable columns)
    assert!(
        preview.contains("name"),
        "Search should include name column"
    );
    assert!(
        preview.contains("email"),
        "Search should include email column"
    );

    // Should use @@ operator
    assert!(preview.contains("@@"), "Search should use @@ operator");

    // Should combine with OR
    assert!(preview.contains(" OR "), "Search should combine with OR");

    // Should NOT include non-searchable columns
    assert!(
        !preview.contains("password"),
        "Search should not include password column"
    );
    assert!(
        !preview.contains("age"),
        "Search should not include age column"
    );
    assert!(
        !preview.contains("id"),
        "Search should not include id column"
    );

    // Should include search value
    assert!(
        preview.contains("john"),
        "Search should include search value"
    );
}

#[test]
fn test_search_expression_no_searchable_columns() {
    // Create a mock SurrealDB instance
    let client = SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    let db = SurrealDB::new(client);

    // Create a table with NO searchable columns
    let table = Table::new("config", db.clone())
        .with_column(Column::new("id").with_flag(ColumnFlag::IdField))
        .with_column(Column::new("key"))
        .with_column(Column::new("value"));

    let any_table = vantage_table::prelude::AnyTable::new(table);

    // Generate search expression
    let search_expr = db.search_expression(&any_table, "test");

    let preview = search_expr.preview();

    // Should return always-true expression when no searchable columns
    assert_eq!(
        preview, "true",
        "Search with no searchable columns should return 'true'"
    );
}

#[test]
fn test_search_expression_single_searchable_column() {
    // Create a mock SurrealDB instance
    let client = SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    let db = SurrealDB::new(client);

    // Create a table with only ONE searchable column
    let table = Table::new("tags", db.clone())
        .with_column(Column::new("id").with_flag(ColumnFlag::IdField))
        .with_column(
            Column::new("name").with_flags(&[ColumnFlag::TitleField, ColumnFlag::Searchable]),
        );

    let any_table = vantage_table::prelude::AnyTable::new(table);

    // Generate search expression
    let search_expr = db.search_expression(&any_table, "important");

    let preview = search_expr.preview();

    // Should include the searchable column
    assert!(
        preview.contains("name"),
        "Search should include name column"
    );

    // Should use @@ operator
    assert!(preview.contains("@@"), "Search should use @@ operator");

    // Should NOT have OR (only one column)
    assert!(
        !preview.contains(" OR "),
        "Search with single column should not have OR"
    );

    // Should include search value
    assert!(
        preview.contains("important"),
        "Search should include search value"
    );
}
