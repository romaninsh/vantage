//! Lazy expressions will greedily own all the parameters.
//! Lazy expressions implement Expression trait
//! Lazy expressions can be converted into Lazy expression.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    expression::owned::{OwnedExpression, OwnedParameter},
    protocol::{DataSource, Expressive},
    value::IntoValueAsync,
};

#[derive(Debug, Clone)]
pub enum LazyParameter {
    /// Scalar value
    Value(Value),
    /// Anything convertable into a value
    IntoValueAsync(Arc<Box<dyn IntoValueAsync>>),
    /// Any expression can be used
    Expression(Arc<Box<dyn Expressive>>),
    /// Embed OwnedExpression directly
    OwnedExpression(OwnedExpression),
    /// Lazy expressions can be embedded
    LazyExpression(Arc<LazyExpression>),
}

// LazyParameter-specific implementations
impl From<LazyExpression> for LazyParameter {
    fn from(expr: LazyExpression) -> Self {
        LazyParameter::LazyExpression(Arc::new(expr))
    }
}

impl From<Box<dyn Expressive>> for LazyParameter {
    fn from(expr: Box<dyn Expressive>) -> Self {
        LazyParameter::Expression(Arc::new(expr))
    }
}

impl From<Arc<Box<dyn Expressive>>> for LazyParameter {
    fn from(expr: Arc<Box<dyn Expressive>>) -> Self {
        LazyParameter::Expression(expr)
    }
}

impl From<Box<dyn IntoValueAsync>> for LazyParameter {
    fn from(value: Box<dyn IntoValueAsync>) -> Self {
        LazyParameter::IntoValueAsync(Arc::new(value))
    }
}

impl From<Arc<Box<dyn IntoValueAsync>>> for LazyParameter {
    fn from(value: Arc<Box<dyn IntoValueAsync>>) -> Self {
        LazyParameter::IntoValueAsync(value)
    }
}

// Generic implementation: anything that can convert to OwnedParameter can also convert to LazyParameter
impl<T: Into<OwnedParameter>> From<T> for LazyParameter {
    fn from(value: T) -> Self {
        match value.into() {
            OwnedParameter::Value(v) => LazyParameter::Value(v),
            OwnedParameter::OwnedExpression(expr) => LazyParameter::OwnedExpression(expr),
        }
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
                    $param.into()
                ),*
            ]
        )
    };
}

#[async_trait]
impl Expressive for LazyExpression {
    async fn prepare(&self, data_source: &dyn DataSource) -> OwnedExpression {
        let token = "{}";

        let mut param_iter = self.parameters.iter();
        let mut sql = self.template.split(token);

        let mut param_out = Vec::new();
        let mut sql_out: String = String::from(sql.next().unwrap());

        while let Some(param) = param_iter.next() {
            match param {
                LazyParameter::Value(value) => {
                    // Keep as is - convert to OwnedParameter and preserve placeholder
                    param_out.push(OwnedParameter::Value(value.clone()));
                    sql_out.push_str("{}");
                }
                LazyParameter::IntoValueAsync(into_value) => {
                    let value = into_value.into_value_async().await;
                    param_out.push(OwnedParameter::Value(value.clone()));
                    sql_out.push_str("{}");
                }
                LazyParameter::OwnedExpression(expr) => {
                    sql_out.push_str(&expr.template);
                    param_out.extend(expr.parameters.clone());
                }
                LazyParameter::Expression(expr) => {
                    // Recursively flatten and replace placeholder with flattened template
                    let flattened = expr.prepare(data_source).await;
                    sql_out.push_str(&flattened.template);
                    param_out.extend(flattened.parameters);
                }
                LazyParameter::LazyExpression(lazy_expr) => {
                    // Recursively flatten and replace placeholder with flattened template
                    let flattened = lazy_expr.prepare(data_source).await;
                    sql_out.push_str(&flattened.template);
                    param_out.extend(flattened.parameters);
                }
            }
            sql_out.push_str(sql.next().unwrap());
        }

        OwnedExpression::new(sql_out, param_out)
    }
}

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

    impl Into<OwnedExpression> for Identifier {
        fn into(self) -> OwnedExpression {
            expr!(format!("`{}`", self.identifier))
        }
    }

    #[test]
    fn test_basic() {
        let expr = LazyExpression::new(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}".to_string(),
            vec![
                Identifier::new("users").into(),
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
            Identifier::new("users"),
            "sue",
            18,
            true,
            Box::new(lazy_expr!("subquery")) as Box<dyn Expressive>
        );

        let preview = expr.preview();
        assert_eq!(
            preview,
            "SELECT * FROM `users` WHERE name=\"sue\" AND age>18 AND true AND gender in **async()"
        );
    }

    #[test]
    fn test_arc() {
        let other_expr: Arc<Box<dyn Expressive>> = Arc::new(Box::new(expr!("now()")));

        let expr = lazy_expr!(
            "SELECT * FROM {} WHERE gender in ({}, {}, {})",
            Identifier::new("users"),
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
