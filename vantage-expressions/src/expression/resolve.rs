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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::expressive::DeferredFn;

    #[tokio::test]
    async fn resolves_deferred_parameters_at_every_depth() {
        let deferred = || {
            ExpressiveEnum::Deferred(DeferredFn::new(|| {
                Box::pin(async { Ok(ExpressiveEnum::Scalar(7)) })
            }))
        };
        let inner = Expression::new("b = {}", vec![deferred()]);
        let expr = Expression::new(
            "a = {} AND {}",
            vec![deferred(), ExpressiveEnum::Nested(inner)],
        );
        assert_eq!(
            expr.resolve_deferred().await.unwrap().preview(),
            "a = 7 AND b = 7"
        );
    }

    fn from_template(template: &str) -> String {
        format!("tpl:{template}")
    }

    fn deferred_to(param: ExpressiveEnum<String>) -> ExpressiveEnum<String> {
        ExpressiveEnum::Deferred(DeferredFn::new(move || {
            let param = match &param {
                ExpressiveEnum::Scalar(v) => ExpressiveEnum::Scalar(v.clone()),
                ExpressiveEnum::Nested(e) => ExpressiveEnum::Nested(e.clone()),
                ExpressiveEnum::Deferred(d) => ExpressiveEnum::Deferred(d.clone()),
            };
            Box::pin(async move { Ok(param) })
        }))
    }

    #[tokio::test]
    async fn resolve_param_collapses_scalar_deferred_and_nested() {
        let scalar = ExpressiveEnum::Scalar("v".to_string());
        assert_eq!(resolve_param(&scalar, from_template).await.unwrap(), "v");

        let chain = deferred_to(ExpressiveEnum::Nested(Expression::new(
            "{}",
            vec![deferred_to(ExpressiveEnum::Scalar("deep".to_string()))],
        )));
        assert_eq!(resolve_param(&chain, from_template).await.unwrap(), "deep");
    }

    #[tokio::test]
    async fn resolve_param_uses_template_without_a_single_parameter() {
        let none = ExpressiveEnum::Nested(Expression::<String>::new("raw", vec![]));
        assert_eq!(
            resolve_param(&none, from_template).await.unwrap(),
            "tpl:raw"
        );

        let two = ExpressiveEnum::Nested(Expression::new(
            "{} {}",
            vec![
                ExpressiveEnum::Scalar("a".to_string()),
                ExpressiveEnum::Scalar("b".to_string()),
            ],
        ));
        assert_eq!(
            resolve_param(&two, from_template).await.unwrap(),
            "tpl:{} {}"
        );
    }

    fn scalar(v: &str) -> ExpressiveEnum<String> {
        ExpressiveEnum::Scalar(v.to_string())
    }

    #[tokio::test]
    async fn execute_by_resolving_hands_many_parameters_to_the_caller() {
        let none = Expression::<String>::new("raw", vec![]);
        let one = Expression::new("{}", vec![deferred_to(scalar("x"))]);
        let two = Expression::new("{} {}", vec![scalar("a"), scalar("b")]);
        let many = |params: &[ExpressiveEnum<String>]| {
            let n = params.len();
            async move { Ok(format!("many:{n}")) }
        };

        assert_eq!(
            execute_by_resolving(&none, from_template, many)
                .await
                .unwrap(),
            "tpl:raw"
        );
        assert_eq!(
            execute_by_resolving(&one, from_template, many)
                .await
                .unwrap(),
            "x"
        );
        assert_eq!(
            execute_by_resolving(&two, from_template, many)
                .await
                .unwrap(),
            "many:2"
        );
    }

    #[tokio::test]
    async fn execute_strict_unwraps_one_level_and_rejects_the_rest() {
        let none = Expression::<String>::new("raw", vec![]);
        assert_eq!(
            execute_strict(&none, from_template, "test").await.unwrap(),
            "tpl:raw"
        );

        let nested = Expression::new(
            "{}",
            vec![ExpressiveEnum::Nested(Expression::new(
                "{}",
                vec![deferred_to(scalar("v"))],
            ))],
        );
        assert_eq!(
            execute_strict(&nested, from_template, "test")
                .await
                .unwrap(),
            "v"
        );

        let two = Expression::new("{} {}", vec![scalar("a"), scalar("b")]);
        let deep = Expression::new(
            "{}",
            vec![ExpressiveEnum::Nested(Expression::new(
                "{}",
                vec![ExpressiveEnum::Nested(Expression::new(
                    "{}",
                    vec![scalar("v")],
                ))],
            ))],
        );
        let to_nested = Expression::new(
            "{}",
            vec![deferred_to(ExpressiveEnum::Nested(Expression::new(
                "{}",
                vec![scalar("v")],
            )))],
        );
        for expr in [two, deep, to_nested] {
            assert!(execute_strict(&expr, from_template, "test").await.is_err());
        }
    }
}
