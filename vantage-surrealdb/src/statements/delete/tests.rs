use crate::statements::delete::SurrealDelete;
use crate::thing::Thing;
use vantage_expressions::Expressive;

#[test]
fn test_delete_table() {
    let del = SurrealDelete::table("users");
    assert_eq!(del.preview(), "DELETE users");
}

#[test]
fn test_delete_record() {
    let del = SurrealDelete::new(Thing::new("users", "john"));
    assert_eq!(del.preview(), "DELETE users:john");
}

#[test]
fn test_delete_with_condition() {
    let del =
        SurrealDelete::table("users").with_condition(crate::surreal_expr!("active = {}", false));
    assert_eq!(del.preview(), "DELETE users WHERE active = false");
}

#[test]
fn test_delete_with_multiple_conditions() {
    let del = SurrealDelete::table("logs")
        .with_condition(crate::surreal_expr!("level = {}", "debug"))
        .with_condition(crate::surreal_expr!("age > {}", 30i64));
    assert_eq!(
        del.preview(),
        "DELETE logs WHERE level = \"debug\" AND age > 30"
    );
}

#[test]
fn test_delete_identifier_escaping() {
    let del = SurrealDelete::table("SELECT");
    assert_eq!(del.preview(), "DELETE ⟨SELECT⟩");
}

#[test]
fn test_delete_produces_parameterized_expression() {
    let del =
        SurrealDelete::table("users").with_condition(crate::surreal_expr!("score < {}", 10i64));
    let expr = del.expr();
    assert!(expr.template.contains("{}"));
}
