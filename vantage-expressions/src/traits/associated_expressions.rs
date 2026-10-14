use crate::traits::expressive::Expressive;
use crate::{ExprDataSource, Expression};

use std::marker::PhantomData;
use vantage_core::Result;

/// An expression bound to a datasource with a known result type.
///
/// It can be executed directly with [`get`](Self::get), or composed into a
/// larger expression with `expr!("WHERE num > {}", (associated))`.
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
    /// Bind `expr` to `datasource`.
    pub fn new(expr: Expression<T>, datasource: &'a DS) -> Self {
        Self {
            expr,
            datasource,
            _result: PhantomData,
        }
    }

    /// Execute the expression and convert the result with `TryFrom`.
    pub async fn get(&self) -> Result<R>
    where
        R: TryFrom<T>,
        R::Error: Into<vantage_core::VantageError>,
    {
        let raw_result = self.datasource.execute(&self.expr).await?;
        R::try_from(raw_result).map_err(Into::into)
    }

    pub fn expression(&self) -> &Expression<T> {
        &self.expr
    }

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
