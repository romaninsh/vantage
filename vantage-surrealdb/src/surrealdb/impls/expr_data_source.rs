use vantage_core::{Context, Result};
use vantage_expressions::{DeferredFn, ExprDataSource, Expression};

use crate::{AnySurrealType, SurrealType, surrealdb::SurrealDB};

impl ExprDataSource<AnySurrealType> for SurrealDB {
    // TODO: this implementation is too crude, might need to make it more elegant
    async fn execute(&self, expr: &Expression<AnySurrealType>) -> Result<AnySurrealType> {
        let (query_str, params) = self.prepare_query(expr);
        let params_cbor = params.to_cbor();
        let client = self.inner.lock().await;
        let result = client
            .query_cbor(&query_str, params_cbor)
            .await
            .map_err(|e| match e {
                surreal_client::SurrealError::ServerError { code, message } => {
                    vantage_core::error!(
                        "SurrealDB server error",
                        error_code = code,
                        server_message = message
                    )
                }
                _ => {
                    vantage_core::error!(
                        "SurrealDB connection error",
                        error_details = e.to_string()
                    )
                }
            })
            .context(vantage_core::error!(
                "Failed to execute expression",
                expression = query_str.clone()
            ))?;

        // Ensure we have exactly one response (we only send single queries)
        let responses = result
            .as_array()
            .ok_or_else(|| vantage_core::error!("Expected array response from SurrealDB"))?;

        if responses.len() != 1 {
            return Err(vantage_core::error!(
                "Expected single response from SurrealDB",
                response_count = responses.len()
            ));
        }

        let response = responses.first().unwrap();
        let response_map = response
            .as_map()
            .ok_or_else(|| vantage_core::error!("Expected map response from SurrealDB"))?;

        // Check if query was successful by looking at status
        let is_success = response_map
            .iter()
            .find(|(k, _)| matches!(k, ciborium::Value::Text(key) if key == "status"))
            .and_then(|(_, v)| match v {
                ciborium::Value::Text(status) => Some(status == "OK"),
                _ => None,
            })
            .unwrap_or(false);

        if is_success {
            // Extract result from successful response
            let extracted_result = response_map
                .iter()
                .next()
                .map(|(_, v)| v)
                .unwrap_or(&result);

            AnySurrealType::from_cbor(extracted_result).ok_or_else(|| {
                vantage_core::error!("Failed to convert SurrealDB result to AnySurrealType")
            })
        } else {
            // Extract error message from failed response
            let error_message = response_map
                .iter()
                .next()
                .and_then(|(_, v)| match v {
                    ciborium::Value::Text(msg) => Some(msg.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Unknown SurrealDB error".to_string());

            Err(vantage_core::error!(
                "SurrealDB query failed",
                query_error = error_message,
                error_type = "query_execution"
            ))
        }
    }

    fn defer(&self, expr: Expression<AnySurrealType>) -> DeferredFn<AnySurrealType> {
        let client = self.clone();
        DeferredFn::from_fn(move || {
            let client = client.clone();
            let expr = expr.clone();
            Box::pin(async move { client.execute(&expr).await })
        })
    }
}
