use std::sync::Arc;
use vantage_expressions::expr;

#[test]
fn test_arc_immutable_values() {
    // Test with integers
    let shared_var = Arc::new(42i64);
    let expr1 = expr!("hello {}", &shared_var);
    let expr2 = expr!("value is {}", shared_var.clone());

    // Both expressions should show the same value immediately
    assert_eq!(expr1.preview(), "hello 42");
    assert_eq!(expr2.preview(), "value is 42");

    // Test with string
    let shared_str = Arc::new("test".to_string());
    let expr3 = expr!("message: {}", &shared_str);
    assert_eq!(expr3.preview(), "message: \"test\"");
}

#[test]
fn test_arc_with_different_types() {
    // Test with bool
    let shared_bool = Arc::new(true);
    let expr1 = expr!("active: {}", &shared_bool);
    assert_eq!(expr1.preview(), "active: true");

    // Test with i32
    let shared_i32 = Arc::new(123i32);
    let expr2 = expr!("count: {}", &shared_i32);
    assert_eq!(expr2.preview(), "count: 123");

    // Test with f64
    let shared_float = Arc::new(std::f64::consts::PI);
    let expr3 = expr!("pi: {}", &shared_float);
    assert_eq!(expr3.preview(), "pi: 3.141592653589793");
}

#[test]
fn test_multiple_expressions_same_arc() {
    let shared_name = Arc::new("Alice".to_string());

    let greeting = expr!("Hello {}", &shared_name);
    let farewell = expr!("Goodbye {}", &shared_name);
    let question = expr!("How are you, {}?", shared_name.clone());

    assert_eq!(greeting.preview(), "Hello \"Alice\"");
    assert_eq!(farewell.preview(), "Goodbye \"Alice\"");
    assert_eq!(question.preview(), "How are you, \"Alice\"?");
}

#[test]
fn test_arc_reference_vs_clone() {
    let shared_value = Arc::new(999i64);

    // Using reference
    let expr_ref = expr!("ref: {}", &shared_value);

    // Using clone
    let expr_clone = expr!("clone: {}", shared_value.clone());

    assert_eq!(expr_ref.preview(), "ref: 999");
    assert_eq!(expr_clone.preview(), "clone: 999");

    // Original Arc should still be usable
    assert_eq!(*shared_value, 999);
}

#[test]
fn test_nested_arc_expressions() {
    let shared_table = Arc::new("users".to_string());
    let shared_id = Arc::new(42i64);

    let table_expr = expr!("FROM {}", &shared_table);
    let condition_expr = expr!("id = {}", &shared_id);

    let full_query = expr!(
        "SELECT * {} WHERE {}",
        vantage_expressions::IntoExpressive::nested(table_expr),
        vantage_expressions::IntoExpressive::nested(condition_expr)
    );

    assert_eq!(
        full_query.preview(),
        "SELECT * FROM \"users\" WHERE id = 42"
    );
}
