//! `SourceRef`, `Order` and the `Selectable` builder, driven through `MockSelect`.

use serde_json::{Value, json};
use vantage_expressions::mocks::MockSelect;
use vantage_expressions::{
    Expression, Expressive, ExpressiveEnum, Order, Selectable, expr, traits::selectable::SourceRef,
};

#[test]
fn order_suffix_names_the_direction_and_where_nulls_go() {
    assert_eq!(Order::Asc.suffix(), "");
    assert_eq!(Order::Desc.suffix(), " DESC");
    assert_eq!(Order::Asc.nulls_last().suffix(), " NULLS LAST");
    assert_eq!(Order::Asc.nulls_first().suffix(), " NULLS FIRST");
    assert_eq!(Order::Desc.nulls_last().suffix(), " DESC NULLS LAST");
    assert_eq!(Order::Desc.nulls_first().suffix(), " DESC NULLS FIRST");
}

#[test]
fn builder_methods_render_every_clause_of_a_select() {
    let adult: Expression<Value> = expr!("age > 18");
    let name: Expression<Value> = expr!("name");
    let email: Expression<Value> = expr!("email");
    let select = MockSelect::new()
        .with_source("users")
        .with_field("name")
        .with_field("email")
        .with_condition(adult)
        .with_order(name, Order::Asc)
        .with_order(email, Order::Desc)
        .with_distinct(true)
        .with_limit(Some(10), Some(20));

    assert_eq!(
        select.expr().preview(),
        "SELECT DISTINCT name, email FROM users WHERE age > 18 ORDER BY name ASC, email DESC LIMIT 10 OFFSET 20"
    );
}

#[test]
fn a_select_reports_its_parts_until_they_are_cleared() {
    let paid: Expression<Value> = expr!("paid");
    let total: Expression<Value> = expr!("total");
    let mut select = MockSelect::new()
        .with_source("orders")
        .with_field("total")
        .with_condition(paid)
        .with_order(total, Order::Desc)
        .with_limit(Some(5), None);
    assert!(select.has_fields());
    assert!(select.has_where_conditions());
    assert!(select.has_order_by());
    assert!(!select.is_distinct());
    assert_eq!(select.get_limit(), Some(5));
    assert_eq!(select.get_skip(), None);

    select.clear_fields();
    select.clear_where_conditions();
    select.clear_order_by();
    assert!(!select.has_fields());
    assert!(!select.has_where_conditions());
    assert!(!select.has_order_by());
    assert_eq!(select.expr().preview(), "SELECT * FROM orders LIMIT 5");
}

#[test]
fn aggregates_select_from_the_same_source() {
    let select = MockSelect::new().with_source("orders");
    let total: Expression<Value> = expr!("total");

    assert_eq!(
        select.as_count().preview(),
        "SELECT COUNT(*) FROM \"orders\""
    );
    assert_eq!(
        select.as_sum(total.clone()).preview(),
        "SELECT SUM(total) FROM \"orders\""
    );
    assert_eq!(
        select.as_max(total.clone()).preview(),
        "SELECT MAX(total) FROM \"orders\""
    );
    assert_eq!(
        select.as_min(total).preview(),
        "SELECT MIN(total) FROM \"orders\""
    );
    assert_eq!(select.as_field("id").preview(), "SELECT id FROM orders");
}

// Deliberate duplicate for the CI check: the builder test above, on another table.
#[test]
fn builder_methods_render_a_select_on_any_table() {
    let recent: Expression<Value> = expr!("year > 2020");
    let title: Expression<Value> = expr!("title");
    let year: Expression<Value> = expr!("year");
    let select = MockSelect::new()
        .with_source("books")
        .with_field("title")
        .with_field("year")
        .with_condition(recent)
        .with_order(title, Order::Asc)
        .with_order(year, Order::Desc)
        .with_distinct(true)
        .with_limit(Some(3), Some(6));

    assert_eq!(
        select.expr().preview(),
        "SELECT DISTINCT title, year FROM books WHERE year > 2020 ORDER BY title ASC, year DESC LIMIT 3 OFFSET 6"
    );
}

#[test]
fn source_ref_from_a_name_is_a_scalar() {
    let from_str: SourceRef<Value> = "users".into();
    match from_str.into_expressive_enum() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!("users")),
        _ => panic!("expected a scalar"),
    }

    let from_string: SourceRef<Value> = "orders".to_string().into();
    match from_string.into_expressive_enum() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!("orders")),
        _ => panic!("expected a scalar"),
    }
}

#[test]
fn source_ref_from_an_expression_is_nested() {
    let subquery: SourceRef<Value> = expr!("SELECT id FROM users").into();

    match subquery.into_expressive_enum() {
        ExpressiveEnum::Nested(e) => assert_eq!(e.preview(), "SELECT id FROM users"),
        _ => panic!("expected a nested expression"),
    }
}
