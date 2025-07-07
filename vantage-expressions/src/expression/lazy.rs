//! Lazy expressions will greedily own all the parameters.
//! Lazy expressions implement Expression trait
//! Lazy expressions can be converted into Lazy expression.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    expression::owned::OwnedExpression,
    protocol::Expression,
    value::{IntoValue, IntoValueAsync},
};

#[derive(Debug, Clone)]
pub enum LazyParameter {
    /// Scalar value
    Value(Value),
    /// Identifiers are also allowed
    Identifier(String),
    /// Anything convertable into a value
    IntoValueAsync(Arc<Box<dyn IntoValueAsync>>),
    /// Any expression can be used
    Expression(Arc<Box<dyn Expression>>),
    /// Embed OwnedExpression directly
    OwnedExpression(OwnedExpression),
    /// Lazy expressions can be embedded
    LazyExpression(Arc<LazyExpression>),
}

/// Trait for types that can be passed as parameters
pub trait IntoLazyParameter {
    fn into_lazy_parameter(self) -> LazyParameter;
}

/// Implement for all variants of IntoValue
impl<T: IntoValue + 'static> IntoLazyParameter for T {
    fn into_lazy_parameter(self) -> LazyParameter {
        LazyParameter::Value(Box::new(self).into_value())
    }
}

impl IntoLazyParameter for Box<dyn Expression> {
    fn into_lazy_parameter(self) -> LazyParameter {
        LazyParameter::Expression(Arc::new(self))
    }
}

impl IntoLazyParameter for Arc<Box<dyn Expression>> {
    fn into_lazy_parameter(self) -> LazyParameter {
        LazyParameter::Expression(self)
    }
}

impl IntoLazyParameter for Arc<Box<dyn IntoValueAsync>> {
    fn into_lazy_parameter(self) -> LazyParameter {
        LazyParameter::IntoValueAsync(self)
    }
}

impl IntoLazyParameter for Box<dyn IntoValueAsync> {
    fn into_lazy_parameter(self) -> LazyParameter {
        LazyParameter::IntoValueAsync(Arc::new(self))
    }
}

impl IntoLazyParameter for LazyExpression {
    fn into_lazy_parameter(self) -> LazyParameter {
        LazyParameter::LazyExpression(Arc::new(self))
    }
}

impl IntoLazyParameter for OwnedExpression {
    fn into_lazy_parameter(self) -> LazyParameter {
        LazyParameter::OwnedExpression(self)
    }
}

impl IntoLazyParameter for LazyParameter {
    fn into_lazy_parameter(self) -> LazyParameter {
        self
    }
}

/// Lazy expression contains template and Vec of parameters
#[derive(Debug, Clone)]
pub struct LazyExpression {
    pub template: String,
    pub parameters: Vec<LazyParameter>,
}

/// Macro to create lazy expressions with template and parameters
#[macro_export]
macro_rules! lazy_expr {
    // Simple template without parameters: lazy_expr!("age")
    ($template:expr) => {
        $crate::expression::lazy::LazyExpression::new($template.to_string(), vec![])
    };

    // Template with parameters: lazy_expr!("{} > {}", param1, param2)
    ($template:expr, $($param:expr),*) => {
        $crate::expression::lazy::LazyExpression::new(
            $template.to_string(),
            vec![
                $(
                    $crate::expression::lazy::IntoLazyParameter::into_lazy_parameter($param)
                ),*
            ]
        )
    };
}

#[async_trait]
impl Expression for LazyExpression {}

impl LazyExpression {
    /// Create a new Lazy expression with template and parameters
    pub fn new(template: String, parameters: Vec<LazyParameter>) -> Self {
        Self {
            template,
            parameters,
        }
    }

    /// Create expression from vector of values and a delimeter
    pub fn from_vec(vec: Vec<Value>, delimeter: &str) -> Self {
        let template = vec
            .iter()
            .map(|_| "{}")
            .collect::<Vec<&str>>()
            .join(delimeter);

        let parameters = vec.into_iter().map(|v| LazyParameter::Value(v)).collect();

        Self {
            template,
            parameters,
        }
    }

    pub fn preview(&self) -> String {
        let mut preview = self.template.clone();
        for param in &self.parameters {
            let param_str = match param {
                LazyParameter::Value(param) => match param {
                    Value::String(s) => format!("{:?}", s),
                    other => format!("{}", other),
                },
                LazyParameter::Identifier(id) => format!("`{}`", id),
                LazyParameter::IntoValueAsync(_) => "**async()".to_string(),
                LazyParameter::Expression(_) => "**async()".to_string(),

                LazyParameter::OwnedExpression(o) => o.preview(),
                LazyParameter::LazyExpression(l) => l.preview(),
            };
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::expr;

    use super::*;

    #[test]
    fn test_basic() {
        let expr = LazyExpression::new(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}".to_string(),
            vec![
                LazyParameter::Identifier("users".to_string()),
                LazyParameter::Value(json!("sue")),
                LazyParameter::Value(json!(18)),
                LazyParameter::Value(json!(true)),
                LazyParameter::Value(json!(["female", "male", "other"])),
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
        let expr = lazy_expr!(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}",
            LazyParameter::Identifier("users".to_string()),
            "sue",
            18,
            true,
            Box::new(lazy_expr!("subquery")) as Box<dyn Expression>
        );

        let preview = expr.preview();
        assert_eq!(
            preview,
            "SELECT * FROM `users` WHERE name=\"sue\" AND age>18 AND true AND gender in **async()"
        );
    }

    #[test]
    fn test_arc() {
        let other_expr: Arc<Box<dyn Expression>> = Arc::new(Box::new(expr!("now()")));

        let expr = lazy_expr!(
            "SELECT * FROM {} WHERE gender in ({}, {}, {})",
            LazyParameter::Identifier("users".to_string()),
            other_expr,
            expr!("now()"),
            lazy_expr!("lazy_now()")
        );

        let preview = expr.preview();
        assert_eq!(
            preview,
            "SELECT * FROM `users` WHERE gender in (**async(), now(), lazy_now())"
        );
    }

    #[tokio::test]
    async fn test_ref() {
        // Demonstrates reference implementation of fetching mutex values
        // when query is flattened.
        use crate::expression::flatten::DataSourceFlatten;
        use crate::protocol::DataSource;
        use std::sync::Mutex;

        #[derive(Debug)]
        struct MockIntoValueAsync {
            mutex: Arc<Mutex<i32>>,
        }

        #[async_trait]
        impl IntoValueAsync for MockIntoValueAsync {
            async fn into_value_async(&self) -> Value {
                let value = *self.mutex.lock().unwrap();
                Value::Number(value.into())
            }
        }

        struct MockDataSource;
        #[async_trait]
        impl DataSource for MockDataSource {}

        let mutex = Arc::new(Mutex::new(1));
        let async_value = MockIntoValueAsync {
            mutex: mutex.clone(),
        };
        let expr = lazy_expr!(
            "select {}",
            Box::new(async_value) as Box<dyn IntoValueAsync>
        );

        *mutex.lock().unwrap() = 2;

        let data_source: Arc<dyn DataSource> = Arc::new(MockDataSource);
        let flattened = data_source.flatten(&expr).await;
        let preview = flattened.preview();

        assert_eq!(preview, "select 2");
    }
}
