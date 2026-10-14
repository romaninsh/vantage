//! Module for mapping Expression types between different value types recursively

use crate::expression::core::Expression;
use crate::traits::expressive::{DeferredFn, DeferredFuture, ExpressiveEnum};

/// Trait for mapping Expression from one type to another
pub trait ExpressionMapper<From, To> {
    /// Convert `Expression<From>` to `Expression<To>`
    fn map_expression(expr: Expression<From>) -> Expression<To>
    where
        From: Into<To> + Send + Clone + 'static,
        To: Send + 'static;
}

impl<From, To> ExpressionMapper<From, To> for Expression<From> {
    fn map_expression(expr: Expression<From>) -> Expression<To>
    where
        From: Into<To> + Send + Clone + 'static,
        To: Send + 'static,
    {
        Expression::new(
            expr.template,
            expr.parameters
                .into_iter()
                .map(|param| map_expressive_enum(param))
                .collect(),
        )
    }
}

/// Convert `ExpressiveEnum<From>` to `ExpressiveEnum<To>`
fn map_expressive_enum<From, To>(enum_value: ExpressiveEnum<From>) -> ExpressiveEnum<To>
where
    From: Into<To> + Send + Clone + 'static,
    To: Send + 'static,
{
    match enum_value {
        ExpressiveEnum::Scalar(value) => ExpressiveEnum::Scalar(value.into()),
        ExpressiveEnum::Nested(expr) => ExpressiveEnum::Nested(Expression::map_expression(expr)),
        // The conversion runs when the deferred call completes.
        ExpressiveEnum::Deferred(deferred) => ExpressiveEnum::Deferred(map_deferred_fn(deferred)),
    }
}

/// Convert `DeferredFn<From>` to `DeferredFn<To>`
fn map_deferred_fn<From, To>(deferred: DeferredFn<From>) -> DeferredFn<To>
where
    From: Into<To> + Send + Clone + 'static,
    To: Send + 'static,
{
    DeferredFn::new(move || {
        let deferred = deferred.clone();
        Box::pin(async move {
            let result = deferred.call().await?;
            Ok(map_expressive_enum(result))
        }) as DeferredFuture<To>
    })
}

/// Extension trait to add map method directly to Expression
pub trait ExpressionMap<From> {
    /// Map this expression to a different type
    fn map<To>(self) -> Expression<To>
    where
        From: Into<To> + Send + Clone + 'static,
        To: Send + 'static;
}

impl<From> ExpressionMap<From> for Expression<From> {
    fn map<To>(self) -> Expression<To>
    where
        From: Into<To> + Send + Clone + 'static,
        To: Send + 'static,
    {
        Expression::map_expression(self)
    }
}
