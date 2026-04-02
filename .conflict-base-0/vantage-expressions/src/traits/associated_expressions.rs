use crate::traits::expressive::Expressive;
use crate::{ExprDataSource, Expression};

use std::marker::PhantomData;
use vantage_core::Result;

/// An expression with an associated datasource and known return type
///
/// AssociatedExpression combines an Expression<T> with a datasource reference
/// and provides compile-time guarantees about the return type when executed.
/// This enables building expressions like `get_row_count()` that can be both
/// executed directly and composed into larger expressions.
///
/// # Key Features
///
/// 1. **Associated return type** - `get_row_count() -> AssociatedExpression<T, usize>`
/// 2. **Direct execution** - `associated.get() -> Result<usize>`
/// 3. **Expression composition** - Can be used in `expr!("WHERE num > {}", (associated))`
///
/// # Examples
///
/// ## Basic usage
/// ```rust
/// # use vantage_expressions::*;
/// # use vantage_expressions::mocks::MockExprDataSource;
/// # tokio_test::block_on(async {
/// # let ds = MockExprDataSource::new(serde_json::json!(42));
/// // Ergonomic API - no type annotations needed!
/// let count_expr = expr!("SELECT COUNT(*) FROM users");
/// let associated = ds.associate::<usize>(count_expr);
///
/// // Use in other expressions via Expressive trait
/// let filter_expr = expr!("WHERE user_count > {}", (associated));
/// # });
/// ```
///
/// ## Custom return types (NewTypes pattern)
/// Users can implement `TryFrom<T>` for their custom types:
/// ```rust,ignore
/// // Example from vantage-types
/// struct Email {
///     pub name: String,
///     pub domain: String,
/// }
///
/// impl Email {
///     pub fn new(name: &str, domain: &str) -> Self {
///         Self {
///             name: name.to_string(),
///             domain: domain.to_string(),
///         }
///     }
/// }
///
/// impl TryFrom<serde_json::Value> for Email {
///     type Error = vantage_core::VantageError;
///     fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
///         let email_str = value.as_str().ok_or_else(|| vantage_core::VantageError::other("Expected email string"))?;
///         let parts: Vec<&str> = email_str.split('@').collect();
///         if parts.len() != 2 {
///             return Err(vantage_core::VantageError::other("Invalid email format"));
///         }
///         Ok(Email::new(parts[0], parts[1]))
///     }
/// }
///
/// fn get_authenticated_users_email(ds: &impl ExprDataSource<serde_json::Value>) -> AssociatedExpression<'_, _, serde_json::Value, Email> {
///     let query = expr!("SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())");
///     ds.associate::<Email>(query)
/// }
/// ```
pub struct AssociatedExpression<'a, DS, T, R>
where
    DS: ExprDataSource<T>,
{
    expr: Expression<T>,
    datasource: &'a DS,
    _result: PhantomData<R>,
}

impl<'a, DS, T, R> AssociatedExpression<'a, DS, T, R>
where
    DS: ExprDataSource<T>,
{
    /// Create a new AssociatedExpression with an expression and datasource reference
    pub fn new(expr: Expression<T>, datasource: &'a DS) -> Self {
        Self {
            expr,
            datasource,
            _result: PhantomData,
        }
    }

    /// Execute the expression and return the typed result
    pub async fn get(&self) -> Result<R>
    where
        R: TryFrom<T>,
        R::Error: Into<vantage_core::VantageError>,
    {
        let raw_result = self.datasource.execute(&self.expr).await?;
        R::try_from(raw_result).map_err(Into::into)
    }

    /// Get a reference to the underlying expression
    pub fn expression(&self) -> &Expression<T> {
        &self.expr
    }

    /// Get a reference to the datasource
    pub fn datasource(&self) -> &'a DS {
        self.datasource
    }
}

impl<'a, DS, T: Clone, R> Expressive<T> for AssociatedExpression<'a, DS, T, R>
where
    DS: ExprDataSource<T>,
{
    fn expr(&self) -> Expression<T> {
        self.expr.clone()
    }
}

impl<'a, DS, T: Clone, R> Clone for AssociatedExpression<'a, DS, T, R>
where
    DS: ExprDataSource<T>,
{
    fn clone(&self) -> Self {
        Self {
            expr: self.expr.clone(),
            datasource: self.datasource,
            _result: PhantomData,
        }
    }
}

impl<'a, DS, T, R> std::fmt::Debug for AssociatedExpression<'a, DS, T, R>
where
    DS: ExprDataSource<T>,
    T: std::fmt::Debug + std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssociatedExpression")
            .field("expr", &self.expr.preview())
            .field("return_type", &std::any::type_name::<R>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr;
    use crate::mocks::MockBuilder;
    use serde_json::Value;

    // Test helper Email type matching vantage ecosystem
    #[derive(Debug, PartialEq)]
    struct Email {
        pub name: String,
        pub domain: String,
    }

    impl Email {
        pub fn new(name: &str, domain: &str) -> Self {
            Self {
                name: name.to_string(),
                domain: domain.to_string(),
            }
        }
    }

    impl TryFrom<Value> for Email {
        type Error = vantage_core::VantageError;

        fn try_from(value: Value) -> std::result::Result<Self, Self::Error> {
            let email_str = value
                .as_str()
                .ok_or_else(|| vantage_core::VantageError::other("Expected email string"))?;
            let parts: Vec<&str> = email_str.split('@').collect();
            if parts.len() != 2 {
                return Err(vantage_core::VantageError::other("Invalid email format"));
            }
            Ok(Email::new(parts[0], parts[1]))
        }
    }

    // This method returns email of a currently logged-in user. But since we do not know, how this
    // expression will be used - we wouldn't want to execute it too early. Associated Expression
    // is ideal solution.
    fn get_authenticated_users_email(
        ds: &MockBuilder,
    ) -> AssociatedExpression<'_, MockBuilder, Value, Email> {
        let query = expr!(
            "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())"
        );
        ds.associate::<Email>(query)
    }

    #[tokio::test]
    async fn test_email_direct_execution() {
        use crate::mocks::mock_builder;

        let ds = mock_builder::new()
            .on_exact_select(
                "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())",
                serde_json::json!("foo@example.com")
            );
        let email_associated = get_authenticated_users_email(&ds);

        // easy interface to convert associated expressions to values
        let result = email_associated.get().await.unwrap();
        assert_eq!(result.name, "foo");
        assert_eq!(result.domain, "example.com");
    }

    #[tokio::test]
    async fn test_email_in_query_composition() {
        use crate::mocks::mock_builder;

        let ds = mock_builder::new()
            .with_flattening()
            .on_exact_select(
                "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())",
                serde_json::json!("foo@example.com")
            )
            .on_exact_select(
                "SELECT balance FROM accounts WHERE email = SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())",
                serde_json::json!(1250.50)
            );

        let email_associated = get_authenticated_users_email(&ds);

        // Use the associated email to get user's balance
        let balance_query = expr!(
            "SELECT balance FROM accounts WHERE email = {}",
            (email_associated)
        );

        let result = ds.execute(&balance_query).await.unwrap();
        assert_eq!(result, serde_json::json!(1250.50));
    }

    #[test]
    fn test_associated_expression_debug() {
        use crate::mocks::mock_builder;

        let ds = mock_builder::new()
            .on_exact_select(
                "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())",
                serde_json::json!("test@example.com")
            );
        let associated = get_authenticated_users_email(&ds);

        let debug_str = format!("{:?}", associated);
        assert!(debug_str.contains("AssociatedExpression"));
        assert!(debug_str.contains("Email"));
        assert!(debug_str.contains("SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())"));
    }

    #[test]
    fn test_associated_expression_accessors() {
        use crate::mocks::mock_builder;

        let ds = mock_builder::new()
            .on_exact_select(
                "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())",
                serde_json::json!("user@test.com")
            );
        let associated = get_authenticated_users_email(&ds);

        // Test accessors
        assert_eq!(
            associated.expression().preview(),
            "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())"
        );
        assert_eq!(associated.expression().parameters.len(), 0);

        // Datasource reference should be the same
        assert!(std::ptr::eq(associated.datasource(), &ds));
    }

    #[test]
    fn test_ergonomic_associate_method() {
        use crate::mocks::mock_builder;

        let ds = mock_builder::new()
            .on_exact_select(
                "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())",
                serde_json::json!("test@example.org")
            );

        // Test the ergonomic associate method
        let associated = get_authenticated_users_email(&ds);

        // Verify it has the correct type and properties
        assert_eq!(
            associated.expression().preview(),
            "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())"
        );
        assert_eq!(associated.expression().parameters.len(), 0);
        assert!(std::ptr::eq(associated.datasource(), &ds));
    }
}
