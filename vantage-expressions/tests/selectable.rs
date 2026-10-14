//! `SourceRef`: a select source from a name or a subquery.

use serde_json::{Value, json};
use vantage_expressions::{ExpressiveEnum, expr, traits::selectable::SourceRef};

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
