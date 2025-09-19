use serde_json::json;
use std::sync::{Arc, Mutex};
use vantage_expressions::expression::flatten::{Flatten, OwnedExpressionFlattener};
use vantage_expressions::protocol::selectable::Selectable;
use vantage_expressions::{DataSource, Expr, IntoExpressive, OwnedExpression, expr};

#[derive(Debug, Clone)]
struct MockSelect;

impl Selectable for MockSelect {
    fn set_source(&mut self, _source: impl Into<Expr>, _alias: Option<String>) {}
    fn add_field(&mut self, _field: impl Into<String>) {}
    fn add_expression(&mut self, _expression: OwnedExpression, _alias: Option<String>) {}
    fn add_where_condition(&mut self, _condition: OwnedExpression) {}
    fn set_distinct(&mut self, _distinct: bool) {}
    fn add_order_by(&mut self, _field_or_expr: impl Into<Expr>, _ascending: bool) {}
    fn add_group_by(&mut self, _expression: OwnedExpression) {}
    fn set_limit(&mut self, _limit: Option<i64>, _skip: Option<i64>) {}
    fn clear_fields(&mut self) {}
    fn clear_where_conditions(&mut self) {}
    fn clear_order_by(&mut self) {}
    fn clear_group_by(&mut self) {}
    fn has_fields(&self) -> bool {
        false
    }
    fn has_where_conditions(&self) -> bool {
        false
    }
    fn has_order_by(&self) -> bool {
        false
    }
    fn has_group_by(&self) -> bool {
        false
    }
    fn is_distinct(&self) -> bool {
        false
    }
    fn get_limit(&self) -> Option<i64> {
        None
    }
    fn get_skip(&self) -> Option<i64> {
        None
    }
}

impl Into<OwnedExpression> for MockSelect {
    fn into(self) -> OwnedExpression {
        expr!("SELECT * FROM mock")
    }
}

// Helper function for pattern matching against database mock patterns
fn find_matching_pattern(
    patterns: &[(String, serde_json::Value)],
    query: &str,
) -> serde_json::Value {
    patterns
        .iter()
        .find(|(pattern, _)| query.contains(pattern))
        .map(|(_, value)| value.clone())
        .unwrap_or(serde_json::Value::Null)
}

// Helper function for executing deferred parameters and flattening
async fn execute_and_flatten_owned_expression(expr: &OwnedExpression) -> OwnedExpression {
    let mut expr = expr.clone();

    // Execute all deferred parameters
    for param in &mut expr.parameters {
        if let IntoExpressive::Deferred(f) = param {
            *param = f().await;
        }
    }

    // Flatten nested expressions
    let flattener = OwnedExpressionFlattener::new();
    flattener.flatten_nested(&expr)
}

#[test]
fn test_arc_mutex_with_database_execution() {
    use serde_json::Value;
    use std::future::Future;
    use std::pin::Pin;
    use tokio;

    // DataSource implementation for OwnedExpression
    #[derive(Clone)]
    struct MockOwnedDatabase {
        patterns: Vec<(String, Value)>,
    }

    impl MockOwnedDatabase {
        fn new(patterns: Vec<(&str, Value)>) -> Self {
            Self {
                patterns: patterns
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            }
        }
    }

    impl DataSource<OwnedExpression> for MockOwnedDatabase {
        fn select(&self) -> impl Selectable {
            MockSelect
        }

        async fn execute(&self, expr: &OwnedExpression) -> Value {
            let expr = execute_and_flatten_owned_expression(expr).await;

            // Simulate async database query execution
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            let query = expr.preview();
            find_matching_pattern(&self.patterns, &query)
        }

        fn defer(
            &self,
            expr: OwnedExpression,
        ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static
        {
            let db = self.clone();
            move || {
                let db = db.clone();
                let expr = expr.clone();
                Box::pin(async move { db.execute(&expr).await })
            }
        }
    }

    tokio_test::block_on(async {
        // Create database mock with patterns for different values
        let db = MockOwnedDatabase::new(vec![
            ("hello 10", json!("greeting_10")),
            ("select spelling from numbers where num=10", json!("ten")),
            (
                "select spelling from numbers where num=15",
                json!("fifteen"),
            ),
        ]);

        // Create shared mutable variable
        let shared_var = Arc::new(Mutex::new(10i32));

        // Create expressions using OwnedExpression
        let expr1 = expr!("hello {}", &shared_var);
        let expr2 = expr!("select spelling from numbers where num={}", &shared_var);

        // Execute first query
        let result1 = db.execute(&expr1).await;
        assert_eq!(result1, json!("greeting_10"));

        let result2_before = db.execute(&expr2).await;
        assert_eq!(result2_before, json!("ten"));

        // Modify the shared value
        {
            let mut guard = shared_var.lock().unwrap();
            *guard = 15;
        }

        // Execute same expression again - deferred evaluation will see new value
        let result2_after = db.execute(&expr2).await;
        assert_eq!(result2_after, json!("fifteen"));
    });
}

#[test]
fn test_arc_mutex_with_nested_expression() {
    use serde_json::Value;
    use std::future::Future;
    use std::pin::Pin;
    use tokio;

    #[derive(Debug, Clone)]
    struct GreetingQuery {
        name: String,
    }

    impl From<&GreetingQuery> for OwnedExpression {
        fn from(greeting: &GreetingQuery) -> OwnedExpression {
            expr!("Hello {}", greeting.name.clone())
        }
    }

    impl From<GreetingQuery> for IntoExpressive<OwnedExpression> {
        fn from(greeting: GreetingQuery) -> Self {
            IntoExpressive::nested(OwnedExpression::from(&greeting))
        }
    }

    // DataSource implementation
    #[derive(Clone)]
    struct MockDatabase {
        patterns: Vec<(String, Value)>,
    }

    impl MockDatabase {
        fn new(patterns: Vec<(&str, Value)>) -> Self {
            Self {
                patterns: patterns
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            }
        }
    }

    impl DataSource<OwnedExpression> for MockDatabase {
        fn select(&self) -> impl Selectable {
            MockSelect
        }

        async fn execute(&self, expr: &OwnedExpression) -> Value {
            let expr = execute_and_flatten_owned_expression(expr).await;

            // Simulate async database query execution
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            let query = expr.preview();
            find_matching_pattern(&self.patterns, &query)
        }

        fn defer(
            &self,
            expr: OwnedExpression,
        ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static
        {
            let db = self.clone();
            move || {
                let db = db.clone();
                let expr = expr.clone();
                Box::pin(async move { db.execute(&expr).await })
            }
        }
    }

    tokio_test::block_on(async {
        // Create database mock
        let db = MockDatabase::new(vec![
            ("select Hello \"world\"", json!("greeting_world")),
            ("select Hello \"vantage\"", json!("greeting_vantage")),
        ]);

        // Create mutable greeting struct
        let greeting = Arc::new(Mutex::new(GreetingQuery {
            name: "world".to_string(),
        }));

        let expr = expr!("select {}", &greeting);

        // Execute first query
        let result1 = db.execute(&expr).await;
        assert_eq!(result1, json!("greeting_world"));

        // Modify the greeting name
        {
            let mut guard = greeting.lock().unwrap();
            guard.name = "vantage".to_string();
        }

        // Execute same expression again - should see new nested expression result
        let result2 = db.execute(&expr).await;
        assert_eq!(result2, json!("greeting_vantage"));
    });
}
