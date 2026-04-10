use surreal_client::{MockSurrealEngine, SurrealClient};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_table::prelude::*;
use vantage_types::EmptyEntity;

fn make_db() -> SurrealDB {
    let client = SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test_db".to_string()),
        Some("test_ns".to_string()),
    );
    SurrealDB::new(client)
}

#[test]
fn test_search_expression_basic() {
    let db = make_db();

    let table = Table::<SurrealDB, EmptyEntity>::new("users", db.clone())
        .with_column(Column::<String>::new("id").with_flag(ColumnFlag::IdField))
        .with_column(
            Column::<String>::new("name")
                .with_flags(&[ColumnFlag::TitleField, ColumnFlag::Searchable]),
        )
        .with_column(Column::<String>::new("email").with_flag(ColumnFlag::Searchable))
        .with_column(Column::<String>::new("password").with_flag(ColumnFlag::Hidden))
        .with_column(Column::<i64>::new("age"));

    // search_table_condition currently returns a simple SEARCH expression
    // TODO: once implementation iterates searchable columns, update assertions
    let search_expr = db.search_table_condition(&table, "john");
    let preview = search_expr.preview();

    assert!(
        preview.contains("SEARCH"),
        "Should contain SEARCH keyword, got: {}",
        preview
    );
    assert!(
        preview.contains("john"),
        "Should contain search value, got: {}",
        preview
    );
}

#[test]
fn test_search_expression_special_characters() {
    let db = make_db();

    let table = Table::<SurrealDB, EmptyEntity>::new("users", db.clone())
        .with_column(Column::<String>::new("id").with_flag(ColumnFlag::IdField))
        .with_column(Column::<String>::new("name").with_flag(ColumnFlag::Searchable));

    let search_expr = db.search_table_condition(&table, "O'Brien");
    let preview = search_expr.preview();

    assert!(
        preview.contains("SEARCH"),
        "Should contain SEARCH keyword, got: {}",
        preview
    );
    assert!(
        preview.contains("O'Brien"),
        "Should contain search value, got: {}",
        preview
    );
}
