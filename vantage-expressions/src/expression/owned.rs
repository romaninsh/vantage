//! Owned expressions will greedily own all the parameters.
//! Owned expressions implement Expressive trait

use serde_json::Value;
use std::sync::Arc;

use crate::protocol::expressive::{Expressive, IntoExpressive};

/// Owned expression contains template and Vec of IntoExpressive parameters
#[derive(Clone)]
pub struct Expression {
    pub template: String,
    pub parameters: Vec<IntoExpressive<Expression>>,
}

impl Expressive<Expression> for Expression {
    fn expr(&self, template: &str, args: Vec<IntoExpressive<Expression>>) -> Expression {
        Expression::new(template, args)
    }
}

impl From<Expression> for IntoExpressive<Expression> {
    fn from(expr: Expression) -> Self {
        IntoExpressive::Nested(expr)
    }
}

impl std::fmt::Debug for Expression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.preview())
    }
}

// Specialized implementations for Expression

impl<T: Into<IntoExpressive<Expression>>> From<Vec<T>> for IntoExpressive<Expression> {
    fn from(vec: Vec<T>) -> Self {
        let values: Vec<Value> = vec
            .into_iter()
            .map(|item| match item.into() {
                IntoExpressive::Scalar(v) => v,
                IntoExpressive::Nested(expr) => Value::String(expr.preview()),
                IntoExpressive::Deferred(_) => Value::String("**deferred()".to_string()),
            })
            .collect();
        IntoExpressive::Scalar(Value::Array(values))
    }
}

impl<T: Into<IntoExpressive<Expression>> + Clone, const N: usize> From<[T; N]>
    for IntoExpressive<Expression>
{
    fn from(arr: [T; N]) -> Self {
        arr.to_vec().into()
    }
}

impl<F, Fut> From<F> for IntoExpressive<Expression>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Value> + Send + 'static,
{
    fn from(f: F) -> Self {
        let f = Arc::new(f);
        IntoExpressive::deferred(move || {
            let f = f.clone();
            Box::pin(async move { IntoExpressive::Scalar(f().await) })
        })
    }
}

/// Macro to create expressions with template and parameters
#[macro_export]
macro_rules! expr {
    // Simple template without parameters: expr!("age")
    ($template:expr) => {
        $crate::expression::owned::Expression::new($template, vec![])
    };

    // Template with parameters: expr!("{} > {}", param1, param2)
    ($template:expr, $($param:expr),*) => {
        $crate::expression::owned::Expression::new(
            $template,
            vec![
                $(
                    $param.into()
                ),*
            ]
        )
    };
}

impl Expression {
    /// Create a new owned expression with template and parameters
    pub fn new(template: impl Into<String>, parameters: Vec<IntoExpressive<Expression>>) -> Self {
        Self {
            template: template.into(),
            parameters,
        }
    }

    /// Create expression from vector of expressions and a delimeter
    pub fn from_vec(vec: Vec<Expression>, delimeter: &str) -> Self {
        let template = vec
            .iter()
            .map(|_| "{}")
            .collect::<Vec<&str>>()
            .join(delimeter);

        let parameters = vec
            .into_iter()
            .map(|expr| IntoExpressive::nested(expr))
            .collect();

        Self {
            template: template,
            parameters,
        }
    }

    pub fn preview(&self) -> String {
        let mut preview = self.template.clone();
        for param in &self.parameters {
            let param_str = param.preview();
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

#[cfg(test)]
mod tests {

    #[derive(Debug)]
    struct Identifier {
        identifier: String,
    }
    impl Identifier {
        pub fn new(identifier: impl Into<String>) -> Self {
            Self {
                identifier: identifier.into(),
            }
        }
    }

    impl From<Identifier> for Expression {
        fn from(id: Identifier) -> Self {
            Expression::new(&format!("`{}`", id.identifier), vec![])
        }
    }

    impl From<Identifier> for IntoExpressive<Expression> {
        fn from(id: Identifier) -> Self {
            IntoExpressive::nested(Expression::from(id))
        }
    }

    use super::*;

    #[test]
    fn test_basic() {
        let expr = Expression::new(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}",
            vec![
                Identifier::new("users").into(),
                IntoExpressive::from("sue"),
                IntoExpressive::from(18i64),
                IntoExpressive::from(true),
                IntoExpressive::from(["female", "male", "other"]),
            ],
        );

        let preview = expr.preview();
        assert_eq!(
            preview,
            "SELECT * FROM `users` WHERE name=\"sue\" AND age>18 AND true AND gender in [\"female\",\"male\",\"other\"]"
        );
    }

    #[test]
    fn test_expr() {
        let expr = expr!(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}",
            Identifier::new("users"),
            "sue",
            18i64,
            true,
            ["female", "male", "other"]
        );

        let preview = expr.preview();
        assert_eq!(
            preview,
            "SELECT * FROM `users` WHERE name=\"sue\" AND age>18 AND true AND gender in [\"female\",\"male\",\"other\"]"
        );
    }
}
