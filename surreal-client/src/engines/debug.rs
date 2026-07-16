//! Debug wrapper for Engine trait that logs all RPC operations

use async_trait::async_trait;
use ciborium::Value as CborValue;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::live::Notification;
use crate::{Engine, Result};

/// Wrapper around an Engine that logs all RPC operations
pub struct DebugEngine {
    inner: Box<dyn Engine>,
}

impl DebugEngine {
    /// Create a new DebugEngine wrapping an existing engine
    pub fn wrap(engine: Box<dyn Engine>) -> Box<dyn Engine> {
        Box::new(Self { inner: engine })
    }

    /// Log an RPC method call
    fn log_request(&self, method: &str, params: &Value) {
        let params_str = serde_json::to_string(params).unwrap_or_default();
        println!("🔍 Surreal RPC: {} {}", method, params_str);
    }

    /// Log an RPC response
    fn log_response(&self, response: &Value) {
        // Check if response contains error
        let icon = if let Value::Array(results) = response {
            if results
                .iter()
                .any(|r| r.get("status").and_then(|s| s.as_str()) == Some("ERR"))
            {
                "❌"
            } else {
                "✅"
            }
        } else if response.get("error").is_some() {
            "❌"
        } else {
            "✅"
        };

        let response_str = serde_json::to_string(response).unwrap_or_default();
        println!("{} {}", icon, response_str);
    }
}

#[async_trait]
impl Engine for DebugEngine {
    async fn send_message(&mut self, method: &str, params: Value) -> Result<Value> {
        self.log_request(method, &params);
        let response = self.inner.send_message(method, params).await?;
        self.log_response(&response);
        Ok(response)
    }

    async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue> {
        println!("🔍 Surreal CBOR RPC: {} {:?}", method, params);
        let response = self.inner.send_message_cbor(method, params).await?;
        println!("✅ CBOR Response: {:?}", response);
        Ok(response)
    }

    async fn register_live(
        &mut self,
        query_id: &str,
    ) -> Result<mpsc::UnboundedReceiver<Notification>> {
        self.inner.register_live(query_id).await
    }

    async fn unregister_live(&mut self, query_id: &str) {
        self.inner.unregister_live(query_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockEngine;

    #[async_trait]
    impl Engine for MockEngine {
        async fn send_message(&mut self, _method: &str, _params: Value) -> Result<Value> {
            Ok(serde_json::json!({
                "status": "OK",
                "result": []
            }))
        }

        async fn send_message_cbor(
            &mut self,
            _method: &str,
            _params: CborValue,
        ) -> Result<CborValue> {
            Ok(CborValue::Map(vec![
                (
                    CborValue::Text("status".to_string()),
                    CborValue::Text("OK".to_string()),
                ),
                (
                    CborValue::Text("result".to_string()),
                    CborValue::Array(vec![]),
                ),
            ]))
        }
    }

    #[tokio::test]
    async fn test_debug_engine_wraps_correctly() {
        let mock = Box::new(MockEngine);
        let mut debug_engine = DebugEngine::wrap(mock);

        let result = debug_engine
            .send_message("test", serde_json::json!(["param1"]))
            .await;

        assert!(result.is_ok());
    }
}
