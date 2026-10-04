//! [`Expression::resolve_deferred`]: call every deferred parameter before an
//! expression is rendered and sent. [`resolve_param`] collapses one parameter
//! all the way down to a single value.

use std::future::Future;
use std::pin::Pin;

use vantage_core::Result;

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
}
