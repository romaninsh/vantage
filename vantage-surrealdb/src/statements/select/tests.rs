use crate::field::Field;
use crate::select::SurrealSelect;
use crate::select::select_field::SelectField;
use crate::surreal_expr;
use vantage_expressions::traits::selectable::{Order, Selectable};

#[test]
fn test_basic_select() {
    let select = SurrealSelect::new()
        .fields(vec![
            SelectField::new(Field::new("name")),
            SelectField::new(Field::new("set")),
        ])
        .from("users");

    let sql = select.preview();

    assert_eq!(sql, "SELECT name, ⟨set⟩ FROM users");
}

#[test]
fn test_select_all() {
    let select = SurrealSelect::new().from("users");

    let sql = select.preview();

    assert_eq!(sql, "SELECT * FROM users");
}

#[test]
fn test_select_with_where_condition() {
    let select = SurrealSelect::new()
        .from("users")
        .field("name")
        .with_where(surreal_expr!("age > 18"));

    assert_eq!(select.preview(), "SELECT name FROM users WHERE age > 18");
}

#[test]
fn test_select_with_multiple_where_conditions() {
    let select = SurrealSelect::new()
        .from("users")
        .field("name")
        .with_where(surreal_expr!("age > 18"))
        .with_where(surreal_expr!("active = true"));

    assert_eq!(
        select.preview(),
        "SELECT name FROM users WHERE age > 18 AND active = true"
    );
}

#[test]
fn test_select_with_order_by() {
    let select = SurrealSelect::new()
        .from("users")
        .field("name")
        .with_order_by(surreal_expr!("name"), Order::Asc);

    assert_eq!(select.preview(), "SELECT name FROM users ORDER BY name");
}

#[test]
fn test_select_with_order_by_desc() {
    let select = SurrealSelect::new()
        .from("users")
        .field("name")
        .with_order_by(surreal_expr!("created_at"), Order::Desc);

    assert_eq!(
        select.preview(),
        "SELECT name FROM users ORDER BY created_at DESC"
    );
}

#[test]
fn test_select_with_group_by() {
    let select = SurrealSelect::new()
        .from("users")
        .field("department")
        .with_expression(surreal_expr!("count()"), Some("count".to_string()))
        .with_group_by(surreal_expr!("department"));

    assert_eq!(
        select.preview(),
        "SELECT department, count() AS count FROM users GROUP BY department"
    );
}

#[test]
fn test_select_with_group_all() {
    let select = SurrealSelect::new()
        .with_value()
        .with_expression(surreal_expr!("math::max(total)"), None)
        .from("order")
        .with_group_all();

    assert_eq!(
        select.preview(),
        "SELECT VALUE math::max(total) FROM order GROUP ALL"
    );
}

#[test]
fn test_select_with_split() {
    // SPLIT renders after WHERE and before GROUP/ORDER.
    let select = SurrealSelect::new()
        .from("product")
        .with_expression(surreal_expr!("tags"), Some("tag".to_string()))
        .field("price")
        .with_split(surreal_expr!("tags"));

    assert_eq!(
        select.preview(),
        "SELECT tags AS tag, price FROM product SPLIT tags"
    );
}

#[test]
fn test_subquery_wraps_and_indexes() {
    use crate::primitives::{index_at, subquery};

    let inner = SurrealSelect::new()
        .with_value()
        .with_expression(surreal_expr!("math::max(total)"), None)
        .from("order")
        .with_group_all();

    // (SELECT …) composes with the [n] indexer just like any expression.
    let wrapped = index_at(subquery(inner), 0);
    assert_eq!(
        wrapped.preview(),
        "(SELECT VALUE math::max(total) FROM order GROUP ALL)[0]"
    );
}

#[test]
fn test_group_all_takes_precedence_over_group_by() {
    // Both set → GROUP ALL wins (it is the more specific request).
    let select = SurrealSelect::new()
        .from("order")
        .field("total")
        .with_group_by(surreal_expr!("client"))
        .with_group_all();

    assert_eq!(select.preview(), "SELECT total FROM order GROUP ALL");
}

#[test]
fn test_select_with_limit() {
    let select = SurrealSelect::new()
        .from("users")
        .field("name")
        .with_limit(10);

    assert_eq!(select.preview(), "SELECT name FROM users LIMIT 10");
}

#[test]
fn test_select_with_limit_and_start() {
    let select = SurrealSelect::new()
        .from("users")
        .field("name")
        .with_limit(10)
        .with_skip(20);

    assert_eq!(select.preview(), "SELECT name FROM users LIMIT 10 START 20");
}

#[test]
fn test_complex_select_query() {
    let select = SurrealSelect::new()
        .from("orders")
        .field("customer_id")
        .with_expression(
            surreal_expr!("SUM(total)"),
            Some("total_amount".to_string()),
        )
        .with_where(surreal_expr!("status = 'completed'"))
        .with_group_by(surreal_expr!("customer_id"))
        .with_order_by(surreal_expr!("total_amount"), Order::Desc)
        .with_limit(5);

    assert_eq!(
        select.preview(),
        "SELECT customer_id, SUM(total) AS total_amount FROM orders WHERE status = 'completed' GROUP BY customer_id ORDER BY total_amount DESC LIMIT 5"
    );
}

#[test]
fn test_selectable_trait_methods() {
    let mut select = SurrealSelect::new();

    // Test Selectable trait methods
    select.add_source("users", None);
    select.add_field("name".to_string());
    select.add_field("email".to_string());
    select.add_expression(surreal_expr!("age * 2"));
    select.add_where_condition(surreal_expr!("age > 18"));
    select.add_order_by(surreal_expr!("name"), Order::Asc);
    select.add_group_by(surreal_expr!("department"));
    select.set_limit(Some(10), Some(5));
    select.set_distinct(true);

    // Test trait query methods
    assert!(select.has_fields());
    assert!(select.has_where_conditions());
    assert!(select.has_order_by());
    assert!(select.has_group_by());
    assert!(select.is_distinct());
    assert_eq!(select.get_limit(), Some(10));
    assert_eq!(select.get_skip(), Some(5));

    // Test clear methods
    select.clear_fields();
    select.clear_where_conditions();
    select.clear_order_by();
    select.clear_group_by();

    assert!(!select.has_fields());
    assert!(!select.has_where_conditions());
    assert!(!select.has_order_by());
    assert!(!select.has_group_by());
}
