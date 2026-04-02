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

/// Convert ExpressiveEnum<From> to ExpressiveEnum<To>
fn map_expressive_enum<From, To>(enum_value: ExpressiveEnum<From>) -> ExpressiveEnum<To>
where
    From: Into<To> + Send + Clone + 'static,
    To: Send + 'static,
{
    match enum_value {
        // Scalar values can be converted directly
        ExpressiveEnum::Scalar(value) => ExpressiveEnum::Scalar(value.into()),

        // Nested expressions are converted recursively
        ExpressiveEnum::Nested(expr) => ExpressiveEnum::Nested(Expression::map_expression(expr)),

        // Deferred values need to be wrapped in a conversion closure
        ExpressiveEnum::Deferred(deferred) => ExpressiveEnum::Deferred(map_deferred_fn(deferred)),
    }
}

/// Convert DeferredFn<From> to DeferredFn<To>
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::datasource::{DataSource, ExprDataSource};
    use crate::traits::expressive::{DeferredFn, ExpressiveEnum};
    use serde_json::Value;
    use vantage_core::Result;

    // Mock String database
    #[derive(Clone)]
    struct StringDatabase {
        result: String,
    }

    impl StringDatabase {
        fn new(result: String) -> Self {
            Self { result }
        }
    }

    impl DataSource for StringDatabase {}

    impl ExprDataSource<String> for StringDatabase {
        async fn execute(&self, _expr: &Expression<String>) -> Result<String> {
            Ok(self.result.clone())
        }

        fn defer(&self, _expr: Expression<String>) -> DeferredFn<String>
        where
            String: Clone + Send + Sync + 'static,
        {
            let result = self.result.clone();
            DeferredFn::new(move || {
                let result = result.clone();
                Box::pin(async move { Ok(ExpressiveEnum::Scalar(result)) })
            })
        }
    }

    // Mock JSON database
    #[derive(Clone)]
    struct JsonDatabase {
        result: Value,
    }

    impl JsonDatabase {
        fn new(result: Value) -> Self {
            Self { result }
        }
    }

    impl DataSource for JsonDatabase {}

    impl ExprDataSource<Value> for JsonDatabase {
        async fn execute(&self, _expr: &Expression<Value>) -> Result<Value> {
            Ok(self.result.clone())
        }

        fn defer(&self, _expr: Expression<Value>) -> DeferredFn<Value>
        where
            Value: Clone + Send + Sync + 'static,
        {
            let result = self.result.clone();
            DeferredFn::new(move || {
                let result = result.clone();
                Box::pin(async move { Ok(ExpressiveEnum::Scalar(result)) })
            })
        }
    }

    #[test]
    fn test_scalar_mapping() {
        let string_expr: Expression<String> =
            Expression::new("age > {}", vec![ExpressiveEnum::Scalar("25".to_string())]);
        let value_expr: Expression<Value> = string_expr.map();

        assert_eq!(value_expr.template, "age > {}");
        assert_eq!(value_expr.parameters.len(), 1);
    }

    #[test]
    fn test_nested_mapping() {
        let inner_expr: Expression<String> = Expression::new(
            "status = {}",
            vec![ExpressiveEnum::Scalar("active".to_string())],
        );
        let outer_expr: Expression<String> = Expression::new(
            "SELECT * FROM users WHERE {}",
            vec![ExpressiveEnum::Nested(inner_expr)],
        );

        let mapped_expr: Expression<Value> = outer_expr.map();

        assert_eq!(mapped_expr.template, "SELECT * FROM users WHERE {}");
        assert_eq!(mapped_expr.parameters.len(), 1);
    }

    #[tokio::test]
    async fn test_deferred_mapping() {
        let deferred_string =
            DeferredFn::new(|| Box::pin(async { Ok(ExpressiveEnum::Scalar("test".to_string())) }));

        let string_expr: Expression<String> = Expression::new(
            "SELECT * WHERE name = {}",
            vec![ExpressiveEnum::Deferred(deferred_string)],
        );

        let value_expr: Expression<Value> = string_expr.map();

        assert_eq!(value_expr.template, "SELECT * WHERE name = {}");
        assert_eq!(value_expr.parameters.len(), 1);

        // Test that the deferred function still works after mapping
        if let ExpressiveEnum::Deferred(ref deferred) = value_expr.parameters[0] {
            let result = deferred.call().await.unwrap();
            match result {
                ExpressiveEnum::Scalar(Value::String(s)) => assert_eq!(s, "test"),
                _ => panic!("Expected string value"),
            }
        } else {
            panic!("Expected deferred parameter");
        }
    }

    #[tokio::test]
    async fn test_cross_database_defer_map() {
        // Create databases with incompatible value types
        let db1 = StringDatabase::new("user123,user456".to_string());
        let db2 = JsonDatabase::new(Value::String("processed".to_string()));

        // Create query for db1
        let string_query = Expression::new(
            "SELECT user_ids FROM active_users WHERE department = {}",
            vec![ExpressiveEnum::Scalar("engineering".to_string())],
        );

        // Defer the query from db1
        let deferred_query = db1.defer(string_query);

        // Map the deferred String query to JSON Value and execute on db2
        let mapped_deferred = DeferredFn::new(move || {
            let deferred_query = deferred_query.clone();
            Box::pin(async move {
                let result = deferred_query.call().await?;
                Ok(map_expressive_enum(result))
            })
        });

        let json_expr = Expression::new(
            "PROCESS_USERS({})",
            vec![ExpressiveEnum::Deferred(mapped_deferred)],
        );

        let result = db2.execute(&json_expr).await;
        assert_eq!(result.unwrap(), Value::String("processed".to_string()));
    }
}
