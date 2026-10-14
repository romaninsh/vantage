//! `AssociatedExpression`: an expression bound to a datasource with a typed result.

use serde_json::{Value, json};
use vantage_expressions::{
    AssociatedExpression, ExprDataSource, Expressive, expr,
    mocks::{MockBuilder, mock_builder},
};

const EMAIL_QUERY: &str = "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())";

#[derive(Debug, PartialEq)]
struct Email {
    name: String,
    domain: String,
}

impl TryFrom<Value> for Email {
    type Error = vantage_core::VantageError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let text = value
            .as_str()
            .ok_or_else(|| vantage_core::VantageError::other("Expected email string"))?;
        match text.split_once('@') {
            Some((name, domain)) => Ok(Email {
                name: name.to_string(),
                domain: domain.to_string(),
            }),
            None => Err(vantage_core::VantageError::other("Invalid email format")),
        }
    }
}

fn authenticated_email(ds: &MockBuilder) -> AssociatedExpression<'_, MockBuilder, Value, Email> {
    ds.associate::<Email>(expr!(EMAIL_QUERY))
}

#[tokio::test]
async fn get_executes_and_converts_to_the_result_type() {
    let ds = mock_builder::new().on_exact_select(EMAIL_QUERY, json!("foo@example.com"));

    let email = authenticated_email(&ds).get().await.unwrap();

    assert_eq!(
        email,
        Email {
            name: "foo".to_string(),
            domain: "example.com".to_string()
        }
    );
}

#[tokio::test]
async fn get_fails_when_the_value_does_not_convert() {
    let ds = mock_builder::new().on_exact_select(EMAIL_QUERY, json!("not-an-email"));
    assert!(authenticated_email(&ds).get().await.is_err());

    let ds = mock_builder::new().on_exact_select(EMAIL_QUERY, json!(12));
    assert!(authenticated_email(&ds).get().await.is_err());
}

#[tokio::test]
async fn get_fails_when_the_datasource_fails() {
    let ds = mock_builder::new().on_exact_select("SELECT 1", json!(1));

    assert!(authenticated_email(&ds).get().await.is_err());
}

#[tokio::test]
async fn associated_expression_composes_into_larger_expressions() {
    let ds = mock_builder::new().with_flattening().on_exact_select(
        format!("SELECT balance FROM accounts WHERE email = {EMAIL_QUERY}"),
        json!(1250.5),
    );

    let balance = expr!(
        "SELECT balance FROM accounts WHERE email = {}",
        (authenticated_email(&ds))
    );

    assert_eq!(ds.execute(&balance).await.unwrap(), json!(1250.5));
}

#[test]
fn accessors_expose_the_expression_and_datasource() {
    let ds = mock_builder::new().on_exact_select(EMAIL_QUERY, json!("user@test.com"));
    let associated = authenticated_email(&ds);

    assert_eq!(associated.expression().preview(), EMAIL_QUERY);
    assert!(associated.expression().parameters.is_empty());
    assert!(std::ptr::eq(associated.datasource(), &ds));
    assert_eq!(associated.expr().preview(), EMAIL_QUERY);

    let copy = associated.clone();
    assert_eq!(copy.expression().preview(), EMAIL_QUERY);
    assert!(std::ptr::eq(copy.datasource(), &ds));
}

#[test]
fn debug_shows_the_query_and_the_result_type() {
    let ds = mock_builder::new().on_exact_select(EMAIL_QUERY, json!("a@b.c"));

    let debug = format!("{:?}", authenticated_email(&ds));

    assert!(debug.contains("AssociatedExpression"));
    assert!(debug.contains(EMAIL_QUERY));
    assert!(debug.contains("Email"));
}
