use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_surrealdb::{
    identifier::Identifier,
    operation::RefOperation,
    select::{SurrealSelect, field::Field},
    thing::Thing,
};

#[test]
fn query01() {
    let mut select = SurrealSelect::new();

    // Set the source table
    select.set_source("product", None);
    select.add_where_condition(expr!("bakery = {}", Thing::new("bakery", "hill_valley")));
    select.add_where_condition(expr!("is_deleted = {}", false));
    select.add_order_by("name", true);

    let result = select.preview();

    assert_eq!(
        result,
        "SELECT * FROM product WHERE bakery = bakery:hill_valley AND is_deleted = false ORDER BY name"
    );

    let mut select = SurrealSelect::new();

    select.set_source(
        Thing::new("bakery", "hill_valley").rref("owns", "product"),
        None,
    );
    select.add_where_condition(expr!("is_deleted = {}", false));
    select.add_order_by("name", true);

    let result2 = select.preview();

    assert_eq!(
        result2,
        "SELECT * FROM (bakery:hill_valley->owns->product) WHERE is_deleted = false ORDER BY name"
    );
}

#[test]
fn query02() {}

#[test]
fn query03() {
    let mut select = SurrealSelect::new();

    select.add_field("name");
    select.add_field("price");
    select.add_expression(
        Identifier::new("inventory").dot("stock"),
        Some("stock".to_string()),
    );

    select.set_source("product", None);
    select.add_where_condition(Field::new("is_deleted").eq(false));

    let result = select.preview();
    assert_eq!(
        result,
        "SELECT name, price, inventory.stock AS stock FROM product WHERE is_deleted = false"
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
    let table_expr = "product_table";
    let mut select3 = SurrealSelect::new();
    select3.set_source(table_expr, None);
    let result3 = select3.preview();
    assert_eq!(result3, "SELECT * FROM product_table");
}
