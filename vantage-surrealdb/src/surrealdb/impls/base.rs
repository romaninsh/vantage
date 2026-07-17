use std::sync::Arc;

use indexmap::IndexMap;
use surreal_client::{LiveStream, SurrealClient};
use vantage_core::{Result, error};
use vantage_expressions::{Expression, ExpressionFlattener, Flatten};

use crate::{AnySurrealType, surrealdb::SurrealDB};

impl SurrealDB {
    pub fn new(client: SurrealClient) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(client)),
        }
    }

    /// Start a SurrealDB `LIVE SELECT` on `resource` (a table name) and return
    /// the change-notification stream. Backs the Vista `watch` path. The client
    /// is cloned out so the live RPC doesn't hold the datasource lock while it
    /// waits.
    pub async fn live(&self, resource: &str) -> Result<LiveStream> {
        let client = self.inner.lock().await.clone();
        client.live(resource).await.map_err(|e| {
            error!(
                format!("surrealdb live query failed: {e}"),
                resource = resource
            )
        })
    }

    /// Convert {} placeholders to $_arg1, $_arg2, etc. and extract parameters
    /// which is the preferred way for Surreal querying
    pub(super) fn prepare_query(
        &self,
        expr: &Expression<AnySurrealType>,
    ) -> (String, IndexMap<String, AnySurrealType>) {
        let flattener = ExpressionFlattener::new();
        let flattened = flattener.flatten(expr);

        let mut query = String::new();
        let mut params = IndexMap::new();
        let template_parts: Vec<&str> = flattened.template.split("{}").collect();
        let mut param_counter = 0;

        query.push_str(template_parts[0]);

        for (i, param) in flattened.parameters.iter().enumerate() {
            match param {
                vantage_expressions::ExpressiveEnum::Scalar(s) => {
                    // Only scalar values get parameterized
                    param_counter += 1;
                    let param_name = format!("_arg{}", param_counter);
                    query.push_str(&format!("${}", param_name));
                    params.insert(param_name, s.clone());
                }
                vantage_expressions::ExpressiveEnum::Deferred(_) => {
                    unreachable!("Deferred params should be resolved before prepare_query");
                }
                vantage_expressions::ExpressiveEnum::Nested(_) => {
                    unreachable!("Nested params should be flattened before prepare_query");
                }
            }

            if i + 1 < template_parts.len() {
                query.push_str(template_parts[i + 1]);
            }
        }

        (query, params)
    }

    // pub async fn query(
    //     &self,
    //     query: String,
    //     params: Vec<crate::AnySurrealType>,
    // ) -> Result<Vec<IndexMap<String, crate::AnySurrealType>>> {
    //     let client = self.inner.lock().await;

    //     // Convert AnySurrealType parameters to CBOR

    //     let cbor_params: Vec<CborValue> = params.iter().map(|p| p.to_cbor()).collect();
    //     let cbor_map = if cbor_params.is_empty() {
    //         None
    //     } else {
    //         // Create a CBOR map with parameter names
    //         let mut map = Vec::new();
    //         for (i, cbor_val) in cbor_params.into_iter().enumerate() {
    //             map.push((CborValue::Text(format!("_arg{}", i + 1)), cbor_val));
    //         }
    //         Some(CborValue::Map(map))
    //     };

    //     let cbor_result = client.query_cbor(&query, cbor_map).await?;

    //     // Convert CBOR result to Vec<IndexMap<String, AnySurrealType>>
    //     match cbor_result {
    //         CborValue::Array(arr) => {
    //             return Ok(Vec<AnySurrealType>::from_cbor(arr));
    //             let mut results = Vec::new();
    //             for item in arr {
    //                 if let CborValue::Map(map) = item {
    //                     let mut index_map = IndexMap::new();
    //                     for (k, v) in map {
    //                         let key = match k {
    //                             CborValue::Text(s) => s,
    //                             _ => format!("{:?}", k),
    //                         };
    //                         index_map.insert(key, crate::AnySurrealType::from_cbor(&v));
    //                     }
    //                     results.push(index_map);
    //                 } else {
    //                     // For non-map results, create a single-entry map
    //                     let mut index_map = IndexMap::new();
    //                     index_map.insert(
    //                         "result".to_string(),
    //                         crate::AnySurrealType::from_cbor(&item),
    //                     );
    //                     results.push(index_map);
    //                 }
    //             }
    //             Ok(results)
    //         }
    //         other => {
    //             // For non-array results, create a single-item vector
    //             let mut index_map = IndexMap::new();
    //             index_map.insert(
    //                 "result".to_string(),
    //                 crate::AnySurrealType::from_cbor(&other),
    //             );
    //             Ok(vec![index_map])
    //         }
    //     }
    // }

    // pub async fn query(
    //     &self,
    //     query: String,
    //     params: Vec<AnySurrealType>,
    // ) -> Result<Vec<IndexMap<String, AnySurrealType>>> {
    //     // For test - return mock data with s1 and s2 fields
    //     let mut result = IndexMap::new();
    //     result.insert("s1".to_string(), AnySurrealType::new("hello".to_string()));
    //     result.insert("s2".to_string(), AnySurrealType::new("world".to_string()));
    //     Ok(vec![result])
    // }

    // pub async fn get(&self, into_query: impl Expressive) -> Value {
    //     let result = self.execute(&into_query.expr()).await;
    //     eprintln!("DEBUG: Get result: {:?}", result);
    //     result
    // }

    // pub async fn query(
    //     &self,
    //     query: String,
    //     params: Vec<crate::AnySurrealType>,
    // ) -> Result<Vec<IndexMap<String, crate::AnySurrealType>>> {
    //     let client = self.inner.lock().await;

    //     // Convert AnySurrealType parameters to CBOR
    //     let cbor_params: Vec<CborValue> = params.iter().map(|p| p.cborify()).collect();
    //     let cbor_map = if cbor_params.is_empty() {
    //         None
    //     } else {
    //         // Create a CBOR map with parameter names
    //         let mut map = Vec::new();
    //         for (i, cbor_val) in cbor_params.into_iter().enumerate() {
    //             map.push((CborValue::Text(format!("_arg{}", i + 1)), cbor_val));
    //         }
    //         Some(CborValue::Map(map))
    //     };

    //     let cbor_result = client.query_cbor(&query, cbor_map).await?;

    //     // Convert CBOR result to Vec<IndexMap<String, AnySurrealType>>
    //     match cbor_result {
    //         CborValue::Array(arr) => {
    //             let mut results = Vec::new();
    //             for item in arr {
    //                 if let CborValue::Map(map) = item {
    //                     let mut index_map = IndexMap::new();
    //                     for (k, v) in map {
    //                         let key = match k {
    //                             CborValue::Text(s) => s,
    //                             _ => format!("{:?}", k),
    //                         };
    //                         index_map.insert(key, crate::AnySurrealType::from_cbor(&v));
    //                     }
    //                     results.push(index_map);
    //                 } else {
    //                     // For non-map results, create a single-entry map
    //                     let mut index_map = IndexMap::new();
    //                     index_map.insert(
    //                         "result".to_string(),
    //                         crate::AnySurrealType::from_cbor(&item),
    //                     );
    //                     results.push(index_map);
    //                 }
    //             }
    //             Ok(results)
    //         }
    //         other => {
    //             // For non-array results, create a single-item vector
    //             let mut index_map = IndexMap::new();
    //             index_map.insert(
    //                 "result".to_string(),
    //                 crate::AnySurrealType::from_cbor(&other),
    //             );
    //             Ok(vec![index_map])
    //         }
    //     }
    // }

    // /// Direct CBOR query method for native type support
    // pub async fn query_cbor(&self, query: &str, params: Option<CborValue>) -> Result<CborValue> {
    //     let client = self.inner.lock().await;
    //     let result = client.query_cbor(query, params).await?;
    //     Ok(result)
    // }

    // /// Merge data into a record by ID
    // pub async fn merge(&self, id: &str, data: Value) -> Result<Value> {
    //     let client = self.inner.lock().await;
    //     client.merge(id, data).await
    // }

    // pub fn select(&self) -> SurrealSelect {
    //     SurrealSelect::new()
    // }

    // /// Convert {} placeholders to $_arg1, $_arg2, etc. and extract parameters
    // fn prepare_query(&self, expr: &Expression) -> (String, HashMap<String, Value>) {
    //     let flattener = ExpressionFlattener::new();
    //     let flattened = flattener.flatten(expr);

    //     let mut query = String::new();
    //     let mut params = HashMap::new();
    //     let template_parts: Vec<&str> = flattened.template.split("{}").collect();
    //     let mut param_counter = 0;

    //     query.push_str(template_parts[0]);

    //     for (i, param) in flattened.parameters.iter().enumerate() {
    //         match param {
    //             vantage_expressions::protocol::expressive::IntoExpressive::Scalar(s) => {
    //                 // Only scalar values get parameterized
    //                 param_counter += 1;
    //                 let param_name = format!("_arg{}", param_counter);
    //                 query.push_str(&format!("${}", param_name));
    //                 params.insert(param_name, s.clone());
    //             }
    //             vantage_expressions::protocol::expressive::IntoExpressive::Deferred(_) => {
    //                 // Deferred expressions get parameterized as null for now
    //                 param_counter += 1;
    //                 let param_name = format!("_arg{}", param_counter);
    //                 query.push_str(&format!("${}", param_name));
    //                 params.insert(param_name, Value::Null);
    //             }
    //             vantage_expressions::protocol::expressive::IntoExpressive::Nested(nested) => {
    //                 // Nested expressions get rendered directly into the query
    //                 query.push_str(&nested.preview());
    //             }
    //         }

    //         if i + 1 < template_parts.len() {
    //             query.push_str(template_parts[i + 1]);
    //         }
    //     }

    //     (query, params)
    // }
}
