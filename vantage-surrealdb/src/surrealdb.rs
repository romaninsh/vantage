use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use vantage_expressions::{Expression, Flatten, OwnedExpressionFlattener, protocol::DataSource};

use surreal_client::SurrealClient;
use surreal_client::error::Result;

use crate::SurrealSelect;
use crate::operation::Expressive;

// Create a wrapper for shared SurrealDB state
#[derive(Clone)]
pub struct SurrealDB {
    inner: Arc<tokio::sync::Mutex<SurrealClient>>,
}

impl SurrealDB {
    pub fn new(client: SurrealClient) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(client)),
        }
    }

    pub async fn get(&self, into_query: impl Expressive) -> Value {
        self.execute(&into_query.expr()).await
    }

    pub async fn query(&self, query: String, params: Value) -> Result<Vec<Value>> {
        let client = self.inner.lock().await;
        let result = client.query(&query, Some(params)).await?;

        // Convert single Value to Vec<Value> for compatibility
        match result {
            Value::Array(vec) => Ok(vec),
            other => Ok(vec![other]),
        }
    }

    pub fn select(&self) -> SurrealSelect {
        SurrealSelect::new()
    }

    /// Convert {} placeholders to $_arg1, $_arg2, etc. and extract parameters
    fn prepare_query(&self, expr: &Expression) -> (String, HashMap<String, Value>) {
        let flattener = OwnedExpressionFlattener::new();
        let flattened = flattener.flatten(expr);

        let mut query = String::new();
        let mut params = HashMap::new();
        let template_parts: Vec<&str> = flattened.template.split("{}").collect();
        let mut param_counter = 0;

        query.push_str(template_parts[0]);

        for (i, param) in flattened.parameters.iter().enumerate() {
            match param {
                vantage_expressions::protocol::expressive::IntoExpressive::Scalar(s) => {
                    // Only scalar values get parameterized
                    param_counter += 1;
                    let param_name = format!("_arg{}", param_counter);
                    query.push_str(&format!("${}", param_name));
                    params.insert(param_name, s.clone());
                }
                vantage_expressions::protocol::expressive::IntoExpressive::Deferred(_) => {
                    // Deferred expressions get parameterized as null for now
                    param_counter += 1;
                    let param_name = format!("_arg{}", param_counter);
                    query.push_str(&format!("${}", param_name));
                    params.insert(param_name, Value::Null);
                }
                vantage_expressions::protocol::expressive::IntoExpressive::Nested(nested) => {
                    // Nested expressions get rendered directly into the query
                    query.push_str(&nested.preview());
                }
            }

            if i + 1 < template_parts.len() {
                query.push_str(template_parts[i + 1]);
            }
        }

        (query, params)
    }
}

// Implement DataSource trait for OwnedExpression
impl DataSource<Expression> for SurrealDB {
    fn select(&self) -> impl vantage_expressions::protocol::selectable::Selectable {
        SurrealSelect::new()
    }

    async fn execute(&self, expr: &Expression) -> Value {
        let (query_str, params) = self.prepare_query(expr);
        let params_json = serde_json::to_value(params).unwrap_or(serde_json::json!({}));

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
                Value::Array(extracted_results)
            }
            Err(_) => Value::Null,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        operation::{Expressive, RefOperation},
        select::SurrealSelect,
        thing::Thing,
    };
    use surreal_client::Engine;
    use vantage_expressions::{
        expr,
        protocol::{expressive::IntoExpressive, selectable::Selectable},
    };

    struct MockEngine;

    #[async_trait::async_trait]
    impl Engine for MockEngine {
        async fn send_message(&mut self, _method: &str, _params: Value) -> Result<Value> {
            Ok(serde_json::Value::Null)
        }
    }

    async fn setup_test_db() -> SurrealDB {
        // This is a placeholder - in real usage, create via Connection::connect()
        // For now, we'll create a mock client for testing

        let db = SurrealClient::new(
            Box::new(MockEngine),
            Some("bakery".to_string()),
            Some("v1".to_string()),
        );

        // Connection and authentication would be handled by Connection builder pattern
        // For testing, we'll skip these steps

        SurrealDB::new(db)
    }

    #[tokio::test]
    async fn test_select_with_thing_reference() {
        let shared_db = setup_test_db().await;
        let mut select = SurrealSelect::new();
        select.set_source("product", None);
        select.add_where_condition(expr!("bakery = {}", Thing::new("bakery", "hill_valley")));
        select.add_where_condition(expr!("is_deleted = {}", false));
        select.add_order_by("name", true);

        let result = shared_db.execute(&select.expr()).await;
        println!("✅ Select with Thing reference: {:?}", result);
    }

    #[tokio::test]
    async fn test_select_with_specific_fields() {
        let shared_db = setup_test_db().await;
        let mut select = SurrealSelect::new();
        select.add_field("name");
        select.add_field("price");
        select.set_source("product", None);
        select.add_where_condition(expr!("price > {}", 100));

        let result = shared_db.execute(&select.expr()).await;
        println!("✅ Select with specific fields: {:?}", result);
    }

    #[tokio::test]
    async fn test_select_with_relationship_traversal() {
        let shared_db = setup_test_db().await;
        let mut select = SurrealSelect::new();
        select.set_source(
            Thing::new("bakery", "hill_valley").rref("owns", "product"),
            None,
        );
        select.add_where_condition(expr!("is_deleted = {}", false));
        select.add_order_by("name", true);

        let result = shared_db.execute(&select.expr()).await;
        println!("✅ Select with relationship traversal: {:?}", result);
    }

    #[tokio::test]
    async fn test_select_with_left_relationship() {
        let shared_db = setup_test_db().await;
        let mut select = SurrealSelect::new();
        select.set_source(
            Thing::new("bakery", "hill_valley").lref("belongs_to", "client"),
            None,
        );
        select.add_order_by("name", true);

        let result = shared_db.execute(&select.expr()).await;
        println!("✅ Select with left relationship: {:?}", result);
    }

    #[tokio::test]
    async fn test_complex_nested_query() {
        let shared_db = setup_test_db().await;
        // Build a more complex query similar to the ones in select.rs tests
        let subquery = SurrealSelect::new()
            .with_source("order")
            .with_condition(expr!("status = {}", "completed"))
            .expr();

        let mut main_select = SurrealSelect::new();
        main_select.add_field("name");
        main_select.add_field("email");
        main_select.set_source("client", None);
        main_select.add_where_condition(expr!("id IN ({})", IntoExpressive::nested(subquery)));

        let result = shared_db.execute(&main_select.expr()).await;
        println!("✅ Complex nested query: {:?}", result);
    }

    #[test]
    fn test_prepare_query_conversion() {
        // Create mock client for testing

        let db = SurrealDB::new(SurrealClient::new(Box::new(MockEngine), None, None));

        let expr = expr!(
            "SELECT * FROM product WHERE price > {} AND name = {}",
            100,
            "bread"
        );
        let (query, params) = db.prepare_query(&expr);

        assert_eq!(
            query,
            "SELECT * FROM product WHERE price > $_arg1 AND name = $_arg2"
        );
        assert_eq!(params.len(), 2);
        assert_eq!(params.get("_arg1"), Some(&Value::Number(100.into())));
        assert_eq!(
            params.get("_arg2"),
            Some(&Value::String("bread".to_string()))
        );
    }

    #[test]
    fn test_prepare_query_with_nested_expression() {
        // Create mock client for testing

        let db = SurrealDB::new(SurrealClient::new(Box::new(MockEngine), None, None));

        let nested = expr!("SELECT id FROM client WHERE active = {}", true);
        let main_expr = expr!(
            "SELECT * FROM product WHERE owner IN ({})",
            IntoExpressive::nested(nested)
        );

        let (query, params) = db.prepare_query(&main_expr);

        assert!(query.contains("$_arg"));
        assert!(!params.is_empty());
        println!("Query: {}", query);
        println!("Params: {:?}", params);
    }

    #[tokio::test]
    async fn test_expression_integration() {
        let shared_db = setup_test_db().await;
        // Test direct expression execution
        let query = expr!("SELECT name, price FROM product WHERE price > {}", 200);
        let result = shared_db.execute(&query).await;
        println!("✅ Direct expression execution: {:?}", result);

        // Test with multiple parameters
        let multi_param_query = expr!(
            "SELECT * FROM product WHERE price BETWEEN {} AND {} AND category = {}",
            50,
            200,
            "pastry"
        );
        let result2 = shared_db.execute(&multi_param_query).await;
        println!("✅ Multi-parameter query: {:?}", result2);
    }
}
