//! Lazy expression can be flattened into Owned expression. This would result in
//!  any nested Lazy expressions will also be flattened into Owned
//!  any nested Owned expressions will be embedded
//!  any IntoValueAsync will be executed
//!  dyn Expression - no idea what to do with it ha-ha.
//!

use async_trait::async_trait;
use std::sync::Arc;

use crate::{
    expression::{
        lazy::{LazyExpression, LazyParameter},
        owned::{OwnedExpression, OwnedParameter},
    },
    protocol::DataSource,
};

#[async_trait]
pub trait DataSourceFlatten {
    // override, clone, fix up
    async fn flatten(&self, lazy_expression: &LazyExpression) -> OwnedExpression {
        self.flatten_internal(lazy_expression).await
    }
    async fn flatten_internal(&self, lazy_expression: &LazyExpression) -> OwnedExpression;
}

#[async_trait]
impl DataSourceFlatten for Arc<dyn DataSource> {
    async fn flatten_internal(&self, lazy_expression: &LazyExpression) -> OwnedExpression {
        let token = "{}";

        let mut param_iter = lazy_expression.parameters.iter();
        let mut sql = lazy_expression.template.split(token);

        let mut param_out = Vec::new();
        let mut sql_out: String = String::from(sql.next().unwrap());

        while let Some(param) = param_iter.next() {
            match param {
                LazyParameter::Value(value) => {
                    param_out.push(OwnedParameter::Value(value.clone()));
                    sql_out.push_str("{}");
                }
                LazyParameter::Expression(_expr) => {
                    todo!();
                }
                LazyParameter::OwnedExpression(flattened) => {
                    sql_out.push_str(&flattened.template);
                    param_out.extend(flattened.parameters.clone());
                }
                LazyParameter::LazyExpression(lazy) => {
                    let flattened = self.flatten(lazy.as_ref()).await;
                    sql_out.push_str(&flattened.template);
                    param_out.extend(flattened.parameters);
                }
                LazyParameter::IntoValueAsync(prep_value) => {
                    // Call prepare and treat as value, preserve placeholder
                    let value = prep_value.as_ref().into_value_async().await;
                    param_out.push(OwnedParameter::Value(value));
                    sql_out.push_str("{}");
                }
            }
            sql_out.push_str(sql.next().unwrap());
        }

        OwnedExpression::new(sql_out, param_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{expr, lazy_expr, value::IntoValueAsync};
    use serde_json::{Value, json};

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

    struct MockDataSource;
    #[async_trait]
    impl DataSource for MockDataSource {}

    #[derive(Debug)]
    struct MockIntoValueAsync;
    #[async_trait]
    impl IntoValueAsync for MockIntoValueAsync {
        async fn into_value_async(&self) -> Value {
            json!("hello!")
        }
    }

    #[tokio::test]
    async fn test_flatten_simple() {
        let data_source: Arc<dyn DataSource> = Arc::new(MockDataSource);
        let into_value: Box<dyn IntoValueAsync> = Box::new(MockIntoValueAsync);

        let expr = lazy_expr!(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in ({}, {})",
            Identifier::new("users"),
            "sue",
            18,
            into_value,
            expr!("now()"),
            lazy_expr!("lazy_now()")
        );

        let flattened = data_source.flatten(&expr).await;

        assert_eq!(
            flattened.template,
            "SELECT * FROM `users` WHERE name={} AND age>{} AND {} AND gender in (now(), lazy_now())"
        );
        assert_eq!(
            flattened.preview(),
            "SELECT * FROM `users` WHERE name=\"sue\" AND age>18 AND \"hello!\" AND gender in (now(), lazy_now())"
        );
    }
}
