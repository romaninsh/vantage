use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_surrealdb::select::SurrealSelect;

#[test]
fn query01() {
    let mut select = SurrealSelect::new();

    // Set the source table
    select.set_source(expr!("product"), None);

    // Add WHERE conditions
    select.add_where_condition(expr!("bakery = bakery:hill_valley"));
    select.add_where_condition(expr!("is_deleted = false"));

    // Add ORDER BY
    select.add_order_by(expr!("name"), true);

    let result = select.preview();

    assert_eq!(
        result,
        "SELECT * FROM product WHERE bakery = bakery:hill_valley AND is_deleted = false ORDER BY name"
    );
}

#[test]
fn test_set_source_accepts_string_and_expression() {
    // Test with string literal directly
    let mut select1 = SurrealSelect::new();
    select1.set_source("users", None);
    let result1 = select1.preview();
    assert_eq!(result1, "SELECT * FROM users");

    // Test with String type
    let table_name = String::from("products");
    let mut select2 = SurrealSelect::new();
    select2.set_source(table_name, None);
    let result2 = select2.preview();
    assert_eq!(result2, "SELECT * FROM products");

    // Test with expression
    let table_expr = expr!("product_table");
    let mut select3 = SurrealSelect::new();
    select3.set_source(table_expr, None);
    let result3 = select3.preview();
    assert_eq!(result3, "SELECT * FROM product_table");
}
