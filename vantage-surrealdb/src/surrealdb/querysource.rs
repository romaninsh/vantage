use serde_json::Value;
use vantage_expressions::Expression;
use vantage_table::QuerySource;

use crate::SurrealDB;

impl QuerySource<Expression> for SurrealDB {
    async fn execute(&self, expr: &Expression) -> Value {
        let (query_str, params) = self.prepare_query(expr);
        let params_json = serde_json::to_value(params).unwrap_or(serde_json::json!({}));

        eprintln!("DEBUG: Executing query: {}", query_str);
        eprintln!("DEBUG: Query params: {:?}", params_json);

        match self.query(query_str, params_json).await {
            Ok(results) => {
                // SurrealDB returns query results, each result may have a wrapper
                // Extract the actual data from each result
                let mut extracted_results = Vec::new();

                for result in results {
                    match result {
                        // If result is wrapped in SurrealDB response format
                        Value::Object(ref obj) if obj.contains_key("result") => {
                            if let Some(result_data) = obj.get("result") {
                                match result_data {
                                    Value::Array(arr) => extracted_results.extend(arr.clone()),
                                    other => extracted_results.push(other.clone()),
                                }
                            }
                        }
                        // If result is already unwrapped
                        other => extracted_results.push(other),
                    }
                }

                // Always return array for SELECT queries since they can return multiple records
                let final_result = Value::Array(extracted_results);
                eprintln!("DEBUG: Final execute result: {:?}", final_result);
                final_result
            }
            Err(e) => {
                eprintln!("DEBUG: Query error: {:?}", e);
                Value::Null
            }
        }
    }

    fn defer(
        &self,
        expr: Expression,
    ) -> impl Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>>
    + Send
    + Sync
    + 'static {
        let client = self.clone();
        move || {
            let client = client.clone();
            let expr = expr.clone();
            Box::pin(async move { client.execute(&expr).await })
        }
    }
}
