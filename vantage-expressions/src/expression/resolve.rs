//! [`Expression::resolve_deferred`]: call every deferred parameter before an
//! expression is rendered and sent. [`resolve_param`] collapses one parameter
//! all the way down to a single value; [`execute_by_resolving`] and
//! [`execute_strict`] build an `ExprDataSource::execute` from that for
//! backends without an expression engine.

use std::future::Future;
use std::pin::Pin;

use vantage_core::{Result, error};

use crate::expression::core::Expression;
use crate::traits::expressive::ExpressiveEnum;

/// Collapse an expression parameter to one value.
///
/// A scalar is returned as is, a deferred parameter is called and its result
/// resolved in turn, and a nested expression with exactly one parameter
/// resolves to that parameter. A nested expression with no parameters, or more
/// than one, has no single value, so `from_template` builds one from its
/// template text.
pub fn resolve_param<'a, T>(
    param: &'a ExpressiveEnum<T>,
    from_template: fn(&str) -> T,
) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>
where
    T: Clone + Send + Sync + 'a,
{
    Box::pin(async move {
        match param {
            ExpressiveEnum::Scalar(v) => Ok(v.clone()),
            ExpressiveEnum::Deferred(deferred) => {
                let result = deferred.call().await?;
                match result {
                    ExpressiveEnum::Scalar(v) => Ok(v),
                    other => resolve_param(&other, from_template).await,
                }
            }
            ExpressiveEnum::Nested(expr) => match expr.parameters.as_slice() {
                [single] => resolve_param(single, from_template).await,
                _ => Ok(from_template(&expr.template)),
            },
        }
    })
}

/// Execute `expr` on a backend with no expression engine of its own by
/// collapsing its parameters to one value.
///
/// An expression without parameters becomes `from_template(template)`, and one
/// with a single parameter goes through [`resolve_param`]. An expression with
/// more parameters is passed to `many`, which decides whether and how to
/// resolve them.
pub async fn execute_by_resolving<'a, T, F>(
    expr: &'a Expression<T>,
    from_template: fn(&str) -> T,
    many: impl FnOnce(&'a [ExpressiveEnum<T>]) -> F,
) -> Result<T>
where
    T: Clone + Send + Sync + 'a,
    F: Future<Output = Result<T>>,
{
    match expr.parameters.as_slice() {
        [] => Ok(from_template(&expr.template)),
        [single] => resolve_param(single, from_template).await,
        params => many(params).await,
    }
}

/// Execute `expr` on a backend that unwraps values without recursing.
///
/// An expression without parameters becomes `from_template(template)`. A single
/// parameter must be a scalar, a deferred call that returns a scalar, or a
/// nested expression whose only parameter is one of those two. Every other
/// shape is an error; `backend` names the caller in the error context.
pub async fn execute_strict<T>(
    expr: &Expression<T>,
    from_template: fn(&str) -> T,
    backend: &'static str,
) -> Result<T>
where
    T: Clone + Send + Sync,
{
    match expr.parameters.as_slice() {
        [] => Ok(from_template(&expr.template)),
        [ExpressiveEnum::Nested(inner)] => match inner.parameters.as_slice() {
            [single] => strict_scalar(single, backend).await,
            _ => Err(error!(
                "Nested expression must have exactly one parameter",
                backend = backend
            )),
        },
        [single] => strict_scalar(single, backend).await,
        _ => Err(error!(
            "Multi-parameter expression execution is not supported",
            backend = backend
        )),
    }
}

/// The scalar behind a scalar or deferred parameter.
async fn strict_scalar<T>(param: &ExpressiveEnum<T>, backend: &'static str) -> Result<T>
where
    T: Clone + Send + Sync,
{
    match param {
        ExpressiveEnum::Scalar(v) => Ok(v.clone()),
        ExpressiveEnum::Deferred(deferred) => match deferred.call().await? {
            ExpressiveEnum::Scalar(v) => Ok(v),
            _ => Err(error!(
                "Deferred parameter resolved to a non-scalar",
                backend = backend
            )),
        },
        ExpressiveEnum::Nested(_) => Err(error!(
            "Only one level of expression nesting is supported",
            backend = backend
        )),
    }
}

impl<T: Clone> Expression<T> {
    /// A copy of this expression with each [`Deferred`](ExpressiveEnum::Deferred)
    /// parameter, nested ones included, replaced by what its call returns.
    /// Fails with the first call that fails.
    pub async fn resolve_deferred(&self) -> Result<Expression<T>> {
        let mut parameters = Vec::with_capacity(self.parameters.len());
        for param in &self.parameters {
            parameters.push(match param {
                ExpressiveEnum::Deferred(deferred) => deferred.call().await?,
                ExpressiveEnum::Nested(inner) => {
                    ExpressiveEnum::Nested(Box::pin(inner.resolve_deferred()).await?)
                }
                ExpressiveEnum::Scalar(_) => param.clone(),
            });
        }
        Ok(Expression::new(self.template.clone(), parameters))
    }
}
