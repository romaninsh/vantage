use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

use crate::surreal_client::{
    engine::Engine,
    error::{Result, SurrealError},
    rpc::{RpcMessage, RpcResponse},
};

/// HTTP engine for SurrealDB connectivity
pub struct HttpEngine {
    client: Client,
    base_url: String,
    timeout: Duration,
    namespace: Option<String>,
    database: Option<String>,
    token: Option<String>,
}

impl HttpEngine {
    /// Create a new HTTP engine
    pub fn new(url: String) -> Result<Self> {
        let parsed_url = Url::parse(&url)?;

        // Convert WebSocket URLs to HTTP
        let http_url = match parsed_url.scheme() {
            "ws" => url.replace("ws://", "http://"),
            "wss" => url.replace("wss://", "https://"),
            "http" | "https" => url,
            _ => return Err(SurrealError::Protocol("Invalid URL scheme".to_string())),
        };

        // Remove /rpc suffix if present
        let base_url = if http_url.ends_with("/rpc") {
            http_url[..http_url.len() - 4].to_string()
        } else {
            http_url
        };

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| SurrealError::Network(e))?;

        Ok(Self {
            client,
            base_url,
            timeout: Duration::from_secs(30),
            namespace: None,
            database: None,
            token: None,
            // incremental_id: 0,
        })
    }

    /// Build headers for requests
    fn build_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("Accept".to_string(), "application/json".to_string());

        if let Some(ref token) = self.token {
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
        }

        if let Some(ref ns) = self.namespace {
            headers.insert("Surreal-NS".to_string(), ns.clone());
        }

        if let Some(ref db) = self.database {
            headers.insert("Surreal-DB".to_string(), db.clone());
        }

        headers
    }

    /// Apply headers to a request builder
    fn apply_headers(&self, mut builder: RequestBuilder) -> RequestBuilder {
        for (key, value) in self.build_headers() {
            builder = builder.header(&key, &value);
        }
        builder
    }

    /// Set authentication token
    pub fn set_token(&mut self, token: Option<String>) {
        self.token = token;
    }

    /// Set namespace and database
    pub fn set_namespace_database(&mut self, namespace: Option<String>, database: Option<String>) {
        self.namespace = namespace;
        self.database = database;
    }

    /// Get server status
    pub async fn status(&self) -> Result<u16> {
        let url = format!("{}/status", self.base_url);
        let builder = self.client.get(&url);
        let builder = self.apply_headers(builder);

        let response = builder.send().await.map_err(SurrealError::Network)?;
        Ok(response.status().as_u16())
    }

    /// Get server health
    pub async fn health(&self) -> Result<u16> {
        let url = format!("{}/health", self.base_url);
        let builder = self.client.get(&url);
        let builder = self.apply_headers(builder);

        let response = builder.send().await.map_err(SurrealError::Network)?;
        Ok(response.status().as_u16())
    }

    /// Import SQL content
    pub async fn import(&self, content: &str, username: &str, password: &str) -> Result<Value> {
        let url = format!("{}/import", self.base_url);
        let builder = self
            .client
            .post(&url)
            .basic_auth(username, Some(password))
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(content.to_string());

        let builder = self.apply_headers(builder);

        let response = builder.send().await.map_err(SurrealError::Network)?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(SurrealError::Connection(format!(
                "Import failed: {} - {}",
                status, error_text
            )));
        }

        let body = response.text().await.map_err(SurrealError::Network)?;
        if body.trim().is_empty() {
            Ok(Value::Null)
        } else {
            match serde_json::from_str(&body) {
                Ok(value) => Ok(value),
                Err(_) => Ok(Value::String(body)),
            }
        }
    }

    /// Export database content
    pub async fn export(&self, username: &str, password: &str) -> Result<String> {
        let url = format!("{}/export", self.base_url);
        let builder = self
            .client
            .get(&url)
            .basic_auth(username, Some(password))
            .header("Accept", "text/plain; charset=utf-8");

        let builder = self.apply_headers(builder);

        let response = builder.send().await.map_err(SurrealError::Network)?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(SurrealError::Connection(format!(
                "Export failed: {} - {}",
                status, error_text
            )));
        }

        response.text().await.map_err(SurrealError::Network)
    }

    /// Import ML model
    pub async fn import_ml(
        &self,
        content: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Value> {
        let url = format!("{}/ml/import", self.base_url);
        let mut builder = self
            .client
            .post(&url)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(content.to_string());

        if let (Some(user), Some(pass)) = (username, password) {
            builder = builder.basic_auth(user, Some(pass));
        }

        let builder = self.apply_headers(builder);

        let response = builder.send().await.map_err(SurrealError::Network)?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(SurrealError::Connection(format!(
                "ML import failed: {} - {}",
                status, error_text
            )));
        }

        let body = response.text().await.map_err(SurrealError::Network)?;
        if body.trim().is_empty() {
            Ok(Value::Null)
        } else {
            match serde_json::from_str(&body) {
                Ok(value) => Ok(value),
                Err(_) => Ok(Value::String(body)),
            }
        }
    }

    /// Export ML model
    pub async fn export_ml(
        &self,
        name: &str,
        version: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<String> {
        let url = format!("{}/ml/export/{}/{}", self.base_url, name, version);
        let mut builder = self
            .client
            .get(&url)
            .header("Accept", "text/plain; charset=utf-8");

        if let (Some(user), Some(pass)) = (username, password) {
            builder = builder.basic_auth(user, Some(pass));
        }

        let builder = self.apply_headers(builder);

        let response = builder.send().await.map_err(SurrealError::Network)?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(SurrealError::Connection(format!(
                "ML export failed: {} - {}",
                status, error_text
            )));
        }

        response.text().await.map_err(SurrealError::Network)
    }
}

#[async_trait]
impl Engine for HttpEngine {
    async fn connect(&mut self) -> Result<()> {
        // For HTTP, we just verify the server is reachable
        let status = self.status().await?;
        if status != 200 {
            return Err(SurrealError::Connection(format!(
                "Server returned status: {}",
                status
            )));
        }

        let health = self.health().await?;
        if health != 200 {
            return Err(SurrealError::Connection(format!(
                "Server health check failed: {}",
                health
            )));
        }

        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        // HTTP connections don't need explicit closing
        self.token = None;
        self.namespace = None;
        self.database = None;
        Ok(())
    }

    async fn rpc(&self, message: RpcMessage) -> Result<Value> {
        // Handle special cases that need to modify the engine state
        match message.method.as_str() {
            "use" => {
                // We can't modify self here since rpc takes &self, so we return a special marker
                // The caller (SurrealDB client) will need to handle this case
                let url = format!("{}/rpc", self.base_url);
                let json_body = message.to_json()?;

                let builder = self.client.post(&url).body(json_body);
                let builder = self.apply_headers(builder);

                let response = builder.send().await.map_err(SurrealError::Network)?;

                if !response.status().is_success() {
                    return Err(SurrealError::Connection(format!(
                        "HTTP error: {}",
                        response.status()
                    )));
                }

                let body = response.text().await.map_err(SurrealError::Network)?;
                let rpc_response = RpcResponse::from_json(&body)?;

                rpc_response.into_result()
            }
            _ => {
                let url = format!("{}/rpc", self.base_url);
                let json_body = message.to_json()?;

                let builder = self.client.post(&url).body(json_body);
                let builder = self.apply_headers(builder);

                let response = builder.send().await.map_err(SurrealError::Network)?;

                if !response.status().is_success() {
                    return Err(SurrealError::Connection(format!(
                        "HTTP error: {}",
                        response.status()
                    )));
                }

                let body = response.text().await.map_err(SurrealError::Network)?;
                let rpc_response = RpcResponse::from_json(&body)?;

                rpc_response.into_result()
            }
        }
    }

    fn set_timeout(&mut self, seconds: u64) {
        self.timeout = Duration::from_secs(seconds);
        // Note: reqwest client timeout is set during creation and can't be changed
        // In a production implementation, we might want to recreate the client
    }

    fn get_timeout(&self) -> u64 {
        self.timeout.as_secs()
    }

    async fn ping(&self) -> Result<()> {
        let status = self.status().await?;
        if status == 200 {
            Ok(())
        } else {
            Err(SurrealError::Connection(format!(
                "Ping failed with status: {}",
                status
            )))
        }
    }

    async fn status(&self) -> Result<u16> {
        self.status().await
    }

    async fn health(&self) -> Result<u16> {
        self.health().await
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_engine_creation() {
        let engine = HttpEngine::new("http://localhost:8000".to_string());
        assert!(engine.is_ok());

        let engine = engine.unwrap();
        assert_eq!(engine.base_url, "http://localhost:8000");
    }

    #[test]
    fn test_url_conversion() {
        let engine = HttpEngine::new("ws://localhost:8000".to_string()).unwrap();
        assert_eq!(engine.base_url, "http://localhost:8000");

        let engine = HttpEngine::new("wss://localhost:8000".to_string()).unwrap();
        assert_eq!(engine.base_url, "https://localhost:8000");

        let engine = HttpEngine::new("http://localhost:8000/rpc".to_string()).unwrap();
        assert_eq!(engine.base_url, "http://localhost:8000");
    }

    #[test]
    fn test_headers() {
        let mut engine = HttpEngine::new("http://localhost:8000".to_string()).unwrap();
        engine.set_token(Some("test_token".to_string()));
        engine.set_namespace_database(Some("test_ns".to_string()), Some("test_db".to_string()));

        let headers = engine.build_headers();
        assert_eq!(
            headers.get("Authorization"),
            Some(&"Bearer test_token".to_string())
        );
        assert_eq!(headers.get("Surreal-NS"), Some(&"test_ns".to_string()));
        assert_eq!(headers.get("Surreal-DB"), Some(&"test_db".to_string()));
    }

    #[tokio::test]
    async fn test_http_engine_connection() {
        let mut engine = HttpEngine::new("http://localhost:8000".to_string()).unwrap();

        // This will fail if SurrealDB is not runningly work if SurrealDB is running
        if let Ok(_) = engine.connect().await {
            println!("HTTP engine connected successfully");

            // Test ping
            if let Ok(_) = engine.ping().await {
                println!("HTTP engine ping successful");
            }
        }
    }
}
