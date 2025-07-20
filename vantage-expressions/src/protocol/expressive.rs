use serde_json::Value;
use std::fmt::{Debug, Formatter, Result};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub trait DataSource<T> {
    fn execute(&self, expr: &T) -> impl Future<Output = Value> + Send;

    fn defer(
        &self,
        expr: T,
    ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static;
}

pub enum IntoExpressive<T> {
    Scalar(Value),
    Nested(T),
    Deferred(
        Arc<dyn Fn() -> Pin<Box<dyn Future<Output = IntoExpressive<T>> + Send>> + Send + Sync>,
    ),
}

impl<T: Debug> Debug for IntoExpressive<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            IntoExpressive::Scalar(val) => f.debug_tuple("Scalar").field(val).finish(),
            IntoExpressive::Nested(val) => f.debug_tuple("Nested").field(val).finish(),
            IntoExpressive::Deferred(_) => f.debug_tuple("Deferred").field(&"<closure>").finish(),
        }
    }
}

pub trait Expressive<T>: Debug {
    fn expr(&self, template: &str, args: Vec<IntoExpressive<T>>) -> T;
}

impl<T: Clone> Clone for IntoExpressive<T> {
    fn clone(&self) -> Self {
        match self {
            IntoExpressive::Scalar(val) => IntoExpressive::Scalar(val.clone()),
            IntoExpressive::Nested(expr) => IntoExpressive::Nested(expr.clone()),
            IntoExpressive::Deferred(f) => IntoExpressive::Deferred(f.clone()),
        }
    }
}

// Macro for types that can be used directly with Value constructors
macro_rules! impl_scalar {
    ($($t:ty => $variant:path),* $(,)?) => {
        $(
            impl<T> From<$t> for IntoExpressive<T> {
                fn from(value: $t) -> Self {
                    IntoExpressive::Scalar($variant(value))
                }
            }
        )*
    };
}

// Macro for types that need .into() conversion
macro_rules! impl_scalar_into {
    ($($t:ty => $variant:path),* $(,)?) => {
        $(
            impl<T> From<$t> for IntoExpressive<T> {
                fn from(value: $t) -> Self {
                    IntoExpressive::Scalar($variant(value.into()))
                }
            }
        )*
    };
}

impl_scalar! {
    bool => Value::Bool,
    String => Value::String,
}

impl_scalar_into! {
    &str => Value::String,
    i8 => Value::Number,
    i16 => Value::Number,
    i32 => Value::Number,
    i64 => Value::Number,
    u8 => Value::Number,
    u16 => Value::Number,
    u32 => Value::Number,
}

impl<T> From<f64> for IntoExpressive<T> {
    fn from(value: f64) -> Self {
        IntoExpressive::Scalar(Value::Number(
            serde_json::Number::from_f64(value).unwrap_or_else(|| 0.into()),
        ))
    }
}

impl<T> From<Value> for IntoExpressive<T> {
    fn from(value: Value) -> Self {
        IntoExpressive::Scalar(value)
    }
}

impl<T, E> From<Arc<T>> for IntoExpressive<E>
where
    T: Into<IntoExpressive<E>> + Clone,
{
    fn from(arc: Arc<T>) -> Self {
        let value = arc.as_ref().clone();
        value.into()
    }
}

impl<T, E> From<&Arc<T>> for IntoExpressive<E>
where
    T: Into<IntoExpressive<E>> + Clone,
{
    fn from(arc: &Arc<T>) -> Self {
        let value = arc.as_ref().clone();
        value.into()
    }
}

impl<T> IntoExpressive<T> {
    pub fn nested(value: T) -> Self {
        IntoExpressive::Nested(value)
    }

    pub fn deferred<F>(f: F) -> Self
    where
        F: Fn() -> Pin<Box<dyn Future<Output = IntoExpressive<T>> + Send>> + Send + Sync + 'static,
    {
        IntoExpressive::Deferred(Arc::new(f))
    }
}

impl<T: Debug> IntoExpressive<T> {
    pub fn preview(&self) -> String {
        match self {
            IntoExpressive::Scalar(Value::String(s)) => format!("{:?}", s),
            IntoExpressive::Scalar(other) => format!("{}", other),
            IntoExpressive::Nested(expr) => format!("{:?}", expr),
            IntoExpressive::Deferred(_) => "**deferred()".to_string(),
        }
    }

    pub fn as_scalar(&self) -> Option<&Value> {
        match self {
            IntoExpressive::Scalar(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_nested(&self) -> Option<&T> {
        match self {
            IntoExpressive::Nested(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_deferred(
        &self,
    ) -> Option<
        &Arc<dyn Fn() -> Pin<Box<dyn Future<Output = IntoExpressive<T>> + Send>> + Send + Sync>,
    > {
        match self {
            IntoExpressive::Deferred(f) => Some(f),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Clone)]
    struct ExampleExpression {
        template: String,
        params: Vec<IntoExpressive<ExampleExpression>>,
    }

    impl ExampleExpression {
        fn new(template: &str, params: Vec<IntoExpressive<ExampleExpression>>) -> Self {
            Self {
                template: template.to_string(),
                params,
            }
        }

        fn preview(&self) -> String {
            let mut preview = self.template.clone();
            for param in &self.params {
                let param_str = param.preview();
                preview = preview.replacen("{}", &param_str, 1);
            }
            preview
        }
    }

    impl std::fmt::Debug for ExampleExpression {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.preview())
        }
    }

    impl Expressive<ExampleExpression> for ExampleExpression {
        fn expr(
            &self,
            template: &str,
            args: Vec<IntoExpressive<ExampleExpression>>,
        ) -> ExampleExpression {
            // Create a new expression with the template and arguments
            ExampleExpression::new(template, args)
        }
    }

    impl From<ExampleExpression> for IntoExpressive<ExampleExpression> {
        fn from(expr: ExampleExpression) -> Self {
            IntoExpressive::Nested(expr)
        }
    }

    impl<F, Fut> From<F> for IntoExpressive<ExampleExpression>
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

    macro_rules! example_expr {
        // Simple template without parameters: example_expr!("age")
        ($template:expr) => {
            ExampleExpression::new($template, vec![])
        };

        // Template with parameters: example_expr!("{} > {}", param1, param2)
        ($template:expr, $($param:expr),*) => {
            ExampleExpression::new(
                $template,
                vec![
                    $(
                        $param.into()
                    ),*
                ]
            )
        };
    }

    fn mock_where_clause(
        condition: impl Into<IntoExpressive<ExampleExpression>>,
    ) -> ExampleExpression {
        let condition_param = condition.into();
        ExampleExpression::new("WHERE {}", vec![condition_param])
    }

    fn first(
        input: impl Into<IntoExpressive<ExampleExpression>>,
    ) -> IntoExpressive<ExampleExpression> {
        match input.into() {
            IntoExpressive::Scalar(Value::Array(arr)) => {
                if let Some(first_val) = arr.into_iter().next() {
                    IntoExpressive::Scalar(first_val)
                } else {
                    IntoExpressive::Scalar(Value::Null)
                }
            }
            IntoExpressive::Scalar(_) => IntoExpressive::Scalar(Value::Null),
            IntoExpressive::Nested(n) => {
                let first_expr = n.expr("FIRST({})", vec![IntoExpressive::nested(n.clone())]);
                IntoExpressive::nested(first_expr)
            }
            IntoExpressive::Deferred(inner_fn) => IntoExpressive::deferred(move || {
                let future = inner_fn();
                Box::pin(async move {
                    let result = future.await;
                    match result {
                        IntoExpressive::Scalar(Value::Array(arr)) => {
                            IntoExpressive::Scalar(arr.into_iter().next().unwrap_or(Value::Null))
                        }
                        IntoExpressive::Scalar(_) => IntoExpressive::Scalar(Value::Null),
                        other => other, // Pass through nested or other deferred
                    }
                })
            }),
        }
    }

    #[test]
    fn test_example_expression() {
        let expr = example_expr!("SELECT * FROM users");
        let expressive = IntoExpressive::nested(expr);
        assert_eq!(
            format!("{:?}", expressive.as_nested().unwrap()),
            "SELECT * FROM users"
        );
    }

    #[test]
    fn test_mock_where_clause() {
        let where_expr = mock_where_clause(42i64);
        assert_eq!(format!("{:?}", where_expr), "WHERE 42");
    }

    #[tokio::test]
    async fn test_first() {
        // Test Scalar case with array
        assert_eq!(first(json!([1, 2, 3])).as_scalar().unwrap(), &json!(1));

        // Test Scalar case with non-array (should return null)
        assert_eq!(
            first(json!("not an array")).as_scalar().unwrap(),
            &Value::Null
        );

        // Test Nested case
        assert_eq!(
            format!(
                "{:?}",
                first(example_expr!("SELECT * FROM items"))
                    .as_nested()
                    .unwrap()
            ),
            "FIRST(SELECT * FROM items)"
        );

        // Test Deferred case
        let result = first(|| async { json!([100, 200, 300]) })
            .as_deferred()
            .unwrap()()
        .await;
        if let IntoExpressive::Scalar(value) = result {
            assert_eq!(value, json!(100));
        } else {
            panic!("Expected scalar result");
        }
    }

    #[test]
    fn test_from_i64() {
        let expr: IntoExpressive<String> = 42i64.into();
        if let Value::Number(n) = expr.as_scalar().unwrap() {
            assert_eq!(n.as_i64(), Some(42));
        } else {
            panic!("Expected scalar number");
        }
    }

    #[test]
    fn test_example_expression_with_params() {
        let expr = example_expr!(
            "SELECT * FROM users WHERE id = {} AND name = {} AND department IN ({})",
            42i64,
            "hello",
            example_expr!("subquery")
        );

        assert_eq!(
            expr.preview(),
            "SELECT * FROM users WHERE id = 42 AND name = \"hello\" AND department IN (subquery)"
        );
    }
    // DataSource implementations for testing
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

    impl DataSource<ExampleExpression> for MockDatabase {
        async fn execute(&self, expr: &ExampleExpression) -> Value {
            // Simulate async database query execution
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            let query = expr.preview();
            for (pattern, value) in &self.patterns {
                if query.contains(pattern) {
                    return value.clone();
                }
            }
            Value::Null
        }

        fn defer(
            &self,
            expr: ExampleExpression,
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

    #[tokio::test]
    async fn test_datasource_basic() {
        let db = MockDatabase::new(vec![("SELECT * FROM items", json!([100, 200, 300, 400]))]);
        let expr = example_expr!("SELECT * FROM items");

        let closure = db.defer(expr);
        let result = closure().await;
        assert_eq!(result, json!([100, 200, 300, 400]));
    }

    #[tokio::test]
    async fn test_datasource_with_scalar_mixing() {
        let db = MockDatabase::new(vec![("SELECT COUNT(*) FROM logs", json!(42))]);
        let subquery = example_expr!("SELECT COUNT(*) FROM logs");

        // Mix deferred result with scalar values
        let mixed_query = example_expr!(
            "SELECT * FROM events WHERE log_count = {} AND status = {} AND user_id = {}",
            db.defer(subquery),
            "active",
            42i64
        );

        assert_eq!(
            mixed_query.preview(),
            "SELECT * FROM events WHERE log_count = **deferred() AND status = \"active\" AND user_id = 42"
        );
    }

    #[tokio::test]
    async fn test_nested_queries() {
        let db = MockDatabase::new(vec![("SELECT * FROM items", json!([100, 200, 300, 400]))]);
        let expr = example_expr!("SELECT * FROM items");

        let result = db.execute(&expr).await;
        assert_eq!(result, json!([100, 200, 300, 400]));
    }
}
