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
async fn get_returns_the_converted_value_or_the_error() {
    let ds = mock_builder::new()
        .on_exact_select(EMAIL_QUERY, json!("foo@example.com"))
        .on_exact_select("SELECT 'not-an-email'", json!("not-an-email"));

    let email = authenticated_email(&ds).get().await.unwrap();
    assert_eq!(
        email,
        Email {
            name: "foo".to_string(),
            domain: "example.com".to_string()
        }
    );

    let not_converted = ds
        .associate::<Email>(expr!("SELECT 'not-an-email'"))
        .get()
        .await
        .unwrap_err();
    assert!(
        not_converted.to_string().contains("Invalid email format"),
        "{not_converted}"
    );

    let not_executed = ds
        .associate::<Email>(expr!("SELECT 1"))
        .get()
        .await
        .unwrap_err();
    assert_ne!(not_executed.to_string(), not_converted.to_string());
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
