use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value;

use vantage_expressions::protocol::datasource::DataSource;
use vantage_expressions::{Expression, ExpressionFlattener, Flatten, QuerySource};

use surreal_client::SurrealClient;
use surreal_client::error::Result;

use crate::SurrealSelect;
use crate::operation::Expressive;

pub mod querysource;
pub mod selectsource;
pub mod tablesource;

// Create a wrapper for shared SurrealDB state
#[derive(Clone)]
pub struct SurrealDB {
    inner: Arc<tokio::sync::Mutex<SurrealClient>>,
}

impl DataSource for SurrealDB {}

impl SurrealDB {
    pub fn new(client: SurrealClient) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(client)),
        }
    }

    pub async fn get(&self, into_query: impl Expressive) -> Value {
        let result = self.execute(&into_query.expr()).await;
        eprintln!("DEBUG: Get result: {:?}", result);
        result
    }

    pub async fn query(
        &self,
        query: String,
        params: Vec<crate::AnySurrealType>,
    ) -> Result<Vec<IndexMap<String, crate::AnySurrealType>>> {
        let client = self.inner.lock().await;

        // Convert AnySurrealType parameters to CBOR
        let cbor_params: Vec<CborValue> = params.iter().map(|p| p.cborify()).collect();
        let cbor_map = if cbor_params.is_empty() {
            None
        } else {
            // Create a CBOR map with parameter names
            let mut map = Vec::new();
            for (i, cbor_val) in cbor_params.into_iter().enumerate() {
                map.push((CborValue::Text(format!("_arg{}", i + 1)), cbor_val));
            }
            Some(CborValue::Map(map))
        };

        let cbor_result = client.query_cbor(&query, cbor_map).await?;

        // Convert CBOR result to Vec<IndexMap<String, AnySurrealType>>
        match cbor_result {
            CborValue::Array(arr) => {
                let mut results = Vec::new();
                for item in arr {
                    if let CborValue::Map(map) = item {
                        let mut index_map = IndexMap::new();
                        for (k, v) in map {
                            let key = match k {
                                CborValue::Text(s) => s,
                                _ => format!("{:?}", k),
                            };
                            index_map.insert(key, crate::AnySurrealType::from_cbor(&v));
                        }
                        results.push(index_map);
                    } else {
                        // For non-map results, create a single-entry map
                        let mut index_map = IndexMap::new();
                        index_map.insert(
                            "result".to_string(),
                            crate::AnySurrealType::from_cbor(&item),
                        );
                        results.push(index_map);
                    }
                }
                Ok(results)
            }
            other => {
                // For non-array results, create a single-item vector
                let mut index_map = IndexMap::new();
                index_map.insert(
                    "result".to_string(),
                    crate::AnySurrealType::from_cbor(&other),
                );
                Ok(vec![index_map])
            }
        }
    }

    /// Direct CBOR query method for native type support
    pub async fn query_cbor(&self, query: &str, params: Option<CborValue>) -> Result<CborValue> {
        let client = self.inner.lock().await;
        let result = client.query_cbor(query, params).await?;
        Ok(result)
    }

    /// Helper function to convert JSON to CBOR
    fn json_to_cbor(json: &Value) -> Result<CborValue> {
        match json {
            Value::Null => Ok(CborValue::Null),
            Value::Bool(b) => Ok(CborValue::Bool(*b)),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(CborValue::Integer(i.into()))
                } else if let Some(f) = n.as_f64() {
                    Ok(CborValue::Float(f))
                } else {
                    Err(surreal_client::SurrealError::Protocol("Invalid number".to_string()).into())
                }
            }
            Value::String(s) => Ok(CborValue::Text(s.clone())),
            Value::Array(arr) => {
                let cbor_arr: Result<Vec<CborValue>> = arr.iter().map(Self::json_to_cbor).collect();
                Ok(CborValue::Array(cbor_arr?))
            }
            Value::Object(obj) => {
                let cbor_map: Result<Vec<(CborValue, CborValue)>> = obj
                    .iter()
                    .map(|(k, v)| Ok((CborValue::Text(k.clone()), Self::json_to_cbor(v)?)))
                    .collect();
                Ok(CborValue::Map(cbor_map?))
            }
        }
    }

    /// Helper function to convert CBOR to JSON
    fn cbor_to_json(cbor: &CborValue) -> Result<Value> {
        match cbor {
            CborValue::Null => Ok(Value::Null),
            CborValue::Bool(b) => Ok(Value::Bool(*b)),
            CborValue::Integer(i) => {
                let num = i128::from(*i);
                if let Ok(i64_val) = i64::try_from(num) {
                    Ok(Value::Number(i64_val.into()))
                } else {
                    Ok(Value::String(num.to_string()))
                }
            }
            CborValue::Float(f) => {
                if let Some(num) = serde_json::Number::from_f64(*f) {
                    Ok(Value::Number(num))
                } else {
                    Err(
                        surreal_client::SurrealError::Protocol("Invalid float value".to_string())
                            .into(),
                    )
                }
            }
            CborValue::Text(s) => Ok(Value::String(s.clone())),
            CborValue::Bytes(b) => Ok(Value::String(hex::encode(b))),
            CborValue::Array(arr) => {
                let json_arr: Result<Vec<Value>> = arr.iter().map(Self::cbor_to_json).collect();
                Ok(Value::Array(json_arr?))
            }
            CborValue::Map(map) => {
                let mut json_obj = serde_json::Map::new();
                for (k, v) in map {
                    let key = match k {
                        CborValue::Text(s) => s.clone(),
                        CborValue::Integer(i) => i128::from(*i).to_string(),
                        _ => {
                            return Err(surreal_client::SurrealError::Protocol(
                                "Invalid map key type".to_string(),
                            )
                            .into());
                        }
                    };
                    json_obj.insert(key, Self::cbor_to_json(v)?);
                }
                Ok(Value::Object(json_obj))
            }
            CborValue::Tag(_tag, value) => Self::cbor_to_json(value),
            _ => Ok(Value::String(format!("{:?}", cbor))),
        }
    }

    /// Merge data into a record by ID
    pub async fn merge(&self, id: &str, data: Value) -> Result<Value> {
        let client = self.inner.lock().await;
        client.merge(id, data).await
    }

    pub fn select(&self) -> SurrealSelect {
        SurrealSelect::new()
    }

    /// Convert {} placeholders to $_arg1, $_arg2, etc. and extract parameters
    fn prepare_query(&self, expr: &Expression) -> (String, HashMap<String, Value>) {
        let flattener = ExpressionFlattener::new();
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

    #[tokio::test]
    async fn test_select_with_thing_reference() {
        let shared_db = setup_test_db().await;
        let mut select = SurrealSelect::new();
        select.set_source("product", None);
        select.add_where_condition(expr!("bakery = {}", (Thing::new("bakery", "hill_valley"))));
        select.add_where_condition(expr!("is_deleted = {}", false));
        select.add_order_by(expr!("name"), true);

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
        select.add_order_by(expr!("name"), true);

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
        select.add_order_by(expr!("name"), true);

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
        main_select.add_where_condition(expr!("id IN ({})", (subquery)));

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
        let main_expr = expr!("SELECT * FROM product WHERE owner IN ({})", (nested));

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
