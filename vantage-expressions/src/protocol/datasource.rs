use serde_json::Value;
use std::pin::Pin;

pub trait DataSource<T> {
    fn execute(&self, expr: &T) -> impl Future<Output = Value> + Send;

    fn defer(
        &self,
        expr: T,
    ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static;
}

// mod tests {
//     // DataSource implementations for testing
//     #[derive(Clone)]
//     struct MockDatabase {
//         patterns: Vec<(String, Value)>,
//     }

//     impl MockDatabase {
//         fn new(patterns: Vec<(&str, Value)>) -> Self {
//             Self {
//                 patterns: patterns
//                     .into_iter()
//                     .map(|(k, v)| (k.to_string(), v))
//                     .collect(),
//             }
//         }
//     }

//     impl DataSource<ExampleExpression> for MockDatabase {
//         async fn execute(&self, expr: &ExampleExpression) -> Value {
//             // Simulate async database query execution
//             tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

//             let query = expr.preview();
//             for (pattern, value) in &self.patterns {
//                 if query.contains(pattern) {
//                     return value.clone();
//                 }
//             }
//             Value::Null
//         }

//         fn defer(
//             &self,
//             expr: ExampleExpression,
//         ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static
//         {
//             let db = self.clone();
//             move || {
//                 let db = db.clone();
//                 let expr = expr.clone();
//                 Box::pin(async move { db.execute(&expr).await })
//             }
//         }
//     }

//     #[tokio::test]
//     async fn test_datasource_basic() {
//         let db = MockDatabase::new(vec![("SELECT * FROM items", json!([100, 200, 300, 400]))]);
//         let expr = example_expr!("SELECT * FROM items");

//         let closure = db.defer(expr);
//         let result = closure().await;
//         assert_eq!(result, json!([100, 200, 300, 400]));
//     }

//     #[tokio::test]
//     async fn test_datasource_with_scalar_mixing() {
//         let db = MockDatabase::new(vec![("SELECT COUNT(*) FROM logs", json!(42))]);
//         let subquery = example_expr!("SELECT COUNT(*) FROM logs");

//         // Mix deferred result with scalar values
//         let mixed_query = example_expr!(
//             "SELECT * FROM events WHERE log_count = {} AND status = {} AND user_id = {}",
//             db.defer(subquery),
//             "active",
//             42i64
//         );

//         assert_eq!(
//             mixed_query.preview(),
//             "SELECT * FROM events WHERE log_count = **deferred() AND status = \"active\" AND user_id = 42"
//         );
//     }

//     #[tokio::test]
//     async fn test_nested_queries() {
//         let db = MockDatabase::new(vec![("SELECT * FROM items", json!([100, 200, 300, 400]))]);
//         let expr = example_expr!("SELECT * FROM items");

//         let result = db.execute(&expr).await;
//         assert_eq!(result, json!([100, 200, 300, 400]));
//     }
// }
