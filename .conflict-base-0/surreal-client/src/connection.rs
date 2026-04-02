//! Connection builder for SurrealDB with authentication and engine creation

use crate::{DebugEngine, Engine, Result, SurrealClient, SurrealError, WsCborEngine, WsEngine};

use serde_json::{Value, json};
use url::Url;

/// Connection builder for SurrealDB
#[derive(Default, Debug, Clone)]
pub struct SurrealConnection {
    /// URL to connect to
    pub url: Option<String>,

    /// Namespace to use
    namespace: Option<String>,

    /// Database to use
    database: Option<String>,

    /// Authentication credentials
    auth: Option<AuthParams>,

    /// Whether to check SurrealDB version compatibility
    version_check: bool,

    /// Whether to enable debug mode for query logging
    debug: bool,
}

/// Authentication parameters
#[derive(Debug, Clone)]
pub enum AuthParams {
    /// Root authentication
    Root { username: String, password: String },
    /// Namespace authentication
    Namespace { username: String, password: String },
    /// Database authentication
    Database { username: String, password: String },
    /// Scope authentication
    Scope {
        namespace: String,
        database: String,
        scope: String,
        params: Value,
    },
    /// JWT token authentication
    Token(String),
}

impl SurrealConnection {
    /// Create a new connection builder
    pub fn new() -> Self {
        Self {
            version_check: true,
            debug: false,
            ..Default::default()
        }
    }

    /// Parse connection from DSN string
    pub fn dsn(dsn: impl AsRef<str>) -> Result<Self> {
        let mut conn = Self::new();
        let url = Url::parse(dsn.as_ref())?;

        // Ensure URL has a proper host
        if url.host().is_none() {
            return Err(SurrealError::Connection(
                "URL must have a valid host".to_string(),
            ));
        }

        // Store the URL without user credentials and path/query
        let base_url = format!("{}://{}", url.scheme(), url.host_str().unwrap());
        let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
        let final_url = format!("{}{}", base_url, port);
        conn.url = Some(final_url);

        // Extract user credentials for root auth
        if !url.username().is_empty() {
            let username = url.username().to_string();
            let password = url.password().unwrap_or("").to_string();
            conn.auth = Some(AuthParams::Root { username, password });
        }

        // Extract namespace and database from path segments
        let path_segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();

        if let Some(namespace) = path_segments.first().filter(|s| !s.is_empty()) {
            conn.namespace = Some(namespace.to_string());
        }
        if let Some(database) = path_segments.get(1).filter(|s| !s.is_empty()) {
            conn.database = Some(database.to_string());
        }

        // Parse query parameters
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "namespace" => conn.namespace = Some(value.into_owned()),
                "database" => conn.database = Some(value.into_owned()),
                "version_check" => {
                    conn.version_check = value.parse().unwrap_or(true);
                }
                _ => {}
            }
        }

        Ok(conn)
    }

    /// Set the URL to connect to
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the namespace
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set the database
    pub fn database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Set root authentication
    pub fn auth_root(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.auth = Some(AuthParams::Root {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    /// Set namespace authentication
    pub fn auth_namespace(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.auth = Some(AuthParams::Namespace {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    /// Set database authentication
    pub fn auth_database(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.auth = Some(AuthParams::Database {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    /// Set scope authentication
    pub fn auth_scope(
        mut self,
        namespace: impl Into<String>,
        database: impl Into<String>,
        scope: impl Into<String>,
        params: Value,
    ) -> Self {
        self.auth = Some(AuthParams::Scope {
            namespace: namespace.into(),
            database: database.into(),
            scope: scope.into(),
            params,
        });
        self
    }

    /// Set JWT token authentication
    pub fn auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth = Some(AuthParams::Token(token.into()));
        self
    }

    /// Set version check flag
    pub fn version_check(mut self, check: bool) -> Self {
        self.version_check = check;
        self
    }

    /// Enable debug mode for query logging
    pub fn with_debug(mut self, enabled: bool) -> Self {
        self.debug = enabled;
        self
    }

    // /// Configure connection pool with custom settings
    // pub fn with_pool_config(mut self, config: PoolConfig) -> Self {
    //     self.pool_config = Some(config);
    //     self
    // }

    pub async fn init_ws_engine(&self, engine: &mut dyn Engine) -> Result<()> {
        match self.auth.as_ref().ok_or(SurrealError::Connection(
            "Attempted to connect without auth".to_string(),
        ))? {
            AuthParams::Root { username, password } => {
                engine
                    .send_message(
                        "signin",
                        json!([{
                            "user": username,
                            "pass": password
                        }]),
                    )
                    .await?;
            }
            AuthParams::Namespace { username, password } => {
                engine
                    .send_message("signin", json!([{
                        "user": username,
                        "pass": password,
                        "NS": self.namespace.clone().ok_or(SurrealError::Connection("Namespace is required for namespace auth".to_string())
                    )?}]))
                    .await?;
            }
            AuthParams::Database { username, password } => {
                engine
                    .send_message("signin", json!([{
                        "user": username,
                        "pass": password,
                        "NS": self.namespace.clone().ok_or( SurrealError::Connection("Namespace is required for namespace auth".to_string()) )?,
                        "DB": self.database.clone().ok_or(
                        SurrealError::Connection("Database is required for database auth".to_string())
                    )?}]))
                    .await?;
            }
            _ => {
                return Err(SurrealError::Connection(
                    "Unsupported authentication method for WebSocket".to_string(),
                ));
            }
        }

        // After authentication, set namespace and database
        if let Some(namespace) = &self.namespace {
            engine
                .send_message(
                    "use",
                    json!([namespace, self.database.as_ref().unwrap_or(&String::new())]),
                )
                .await?;
        }

        Ok(())
    }

    pub async fn init_cbor_engine(&self, engine: &mut crate::WsCborEngine) -> Result<()> {
        use ciborium::Value as CborValue;

        match self.auth.as_ref().ok_or(SurrealError::Connection(
            "Attempted to connect without auth".to_string(),
        ))? {
            AuthParams::Root { username, password } => {
                let auth_params = CborValue::Array(vec![CborValue::Map(vec![
                    (
                        CborValue::Text("user".to_string()),
                        CborValue::Text(username.clone()),
                    ),
                    (
                        CborValue::Text("pass".to_string()),
                        CborValue::Text(password.clone()),
                    ),
                ])]);
                engine.send_message_cbor("signin", auth_params).await?;
            }
            AuthParams::Namespace { username, password } => {
                let namespace = self.namespace.clone().ok_or(SurrealError::Connection(
                    "Namespace is required for namespace auth".to_string(),
                ))?;
                let auth_params = CborValue::Array(vec![CborValue::Map(vec![
                    (
                        CborValue::Text("user".to_string()),
                        CborValue::Text(username.clone()),
                    ),
                    (
                        CborValue::Text("pass".to_string()),
                        CborValue::Text(password.clone()),
                    ),
                    (
                        CborValue::Text("NS".to_string()),
                        CborValue::Text(namespace),
                    ),
                ])]);
                engine.send_message_cbor("signin", auth_params).await?;
            }
            AuthParams::Database { username, password } => {
                let namespace = self.namespace.clone().ok_or(SurrealError::Connection(
                    "Namespace is required for database auth".to_string(),
                ))?;
                let database = self.database.clone().ok_or(SurrealError::Connection(
                    "Database is required for database auth".to_string(),
                ))?;
                let auth_params = CborValue::Array(vec![CborValue::Map(vec![
                    (
                        CborValue::Text("user".to_string()),
                        CborValue::Text(username.clone()),
                    ),
                    (
                        CborValue::Text("pass".to_string()),
                        CborValue::Text(password.clone()),
                    ),
                    (
                        CborValue::Text("NS".to_string()),
                        CborValue::Text(namespace),
                    ),
                    (CborValue::Text("DB".to_string()), CborValue::Text(database)),
                ])]);
                engine.send_message_cbor("signin", auth_params).await?;
            }
            _ => {
                return Err(SurrealError::Connection(
                    "Unsupported authentication method for CBOR WebSocket".to_string(),
                ));
            }
        }

        // After authentication, set namespace and database
        if let Some(namespace) = &self.namespace {
            let use_params = CborValue::Array(vec![
                CborValue::Text(namespace.clone()),
                CborValue::Text(self.database.as_ref().unwrap_or(&String::new()).clone()),
            ]);
            engine.send_message_cbor("use", use_params).await?;
        }

        Ok(())
    }

    /// Connect to SurrealDB and return an immutable client
    pub async fn connect(self) -> Result<SurrealClient> {
        let url_str = self
            .url
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("URL is required".to_string()))?;
        let url = Url::parse(url_str)
            .map_err(|e| SurrealError::Connection(format!("Invalid URL: {}", e)))?;

        let mut engine: Box<dyn Engine> = match url.scheme() {
            "ws" | "wss" => Box::new(WsEngine::from_connection(&self).await?),
            "cbor" => {
                let mut cbor_engine = WsCborEngine::from_connection(&self).await?;
                self.init_cbor_engine(&mut cbor_engine).await?;
                Box::new(cbor_engine)
            }
            // "http" | "https" => Box::new(HttpEngine::new(url_str)?),
            _ => {
                return Err(SurrealError::Protocol(
                    "Unsupported protocol. Use ws://, wss://, cbor://, http://, or https://"
                        .to_string(),
                ));
            }
        };

        // Wrap with debug engine if debug mode is enabled
        if self.debug {
            engine = DebugEngine::wrap(engine);
        }

        // Connect to the database
        // engine.connect().await?;
        let client = SurrealClient::new(engine, self.namespace, self.database);
        Ok(client.with_debug(self.debug))
    }

    /*
        // Set namespace and database if provided
        if self.namespace.is_some() || self.database.is_some() {
            let message = crate::surreal_client::RpcMessage::new("use")
                .with_id(1)
                .with_params(vec![
                    self.namespace
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                    self.database
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                ]);

            engine.rpc(message).await?;

            // For HTTP engines, update the namespace/database in the engine
            if let Some(http_engine) = engine.as_any_mut().downcast_mut::<HttpEngine>() {
                http_engine.set_namespace_database(self.namespace.clone(), self.database.clone());
            }
        }

        // Authenticate if credentials provided
        if let Some(auth) = &self.auth {
            match auth {
                AuthParams::Root { username, password } => {
                    let message = crate::surreal_client::RpcMessage::new("signin")
                        .with_id(2)
                        .with_params(vec![Value::Object({
                            let mut map = serde_json::Map::new();
                            map.insert("user".to_string(), Value::String(username.clone()));
                            map.insert("pass".to_string(), Value::String(password.clone()));
                            map
                        })]);

                    let response = engine.rpc(message).await?;

                    // For HTTP engines, set the token if we got one
                    if let Value::String(token) = response {
                        if let Some(http_engine) = engine.as_any_mut().downcast_mut::<HttpEngine>()
                        {
                            http_engine.set_token(Some(token));
                        }
                    }
                }
                AuthParams::Namespace { username, password } => {
                    let message = crate::surreal_client::RpcMessage::new("signin")
                        .with_id(2)
                        .with_params(vec![Value::Object({
                            let mut map = serde_json::Map::new();
                            map.insert(
                                "NS".to_string(),
                                self.namespace
                                    .clone()
                                    .map(Value::String)
                                    .unwrap_or(Value::Null),
                            );
                            map.insert("user".to_string(), Value::String(username.clone()));
                            map.insert("pass".to_string(), Value::String(password.clone()));
                            map
                        })]);

                    let response = engine.rpc(message).await?;

                    if let Value::String(token) = response {
                        if let Some(http_engine) = engine.as_any_mut().downcast_mut::<HttpEngine>()
                        {
                            http_engine.set_token(Some(token));
                        }
                    }
                }
                AuthParams::Database { username, password } => {
                    let message = crate::surreal_client::RpcMessage::new("signin")
                        .with_id(2)
                        .with_params(vec![Value::Object({
                            let mut map = serde_json::Map::new();
                            map.insert(
                                "NS".to_string(),
                                self.namespace
                                    .clone()
                                    .map(Value::String)
                                    .unwrap_or(Value::Null),
                            );
                            map.insert(
                                "DB".to_string(),
                                self.database
                                    .clone()
                                    .map(Value::String)
                                    .unwrap_or(Value::Null),
                            );
                            map.insert("user".to_string(), Value::String(username.clone()));
                            map.insert("pass".to_string(), Value::String(password.clone()));
                            map
                        })]);

                    let response = engine.rpc(message).await?;

                    if let Value::String(token) = response {
                        if let Some(http_engine) = engine.as_any_mut().downcast_mut::<HttpEngine>()
                        {
                            http_engine.set_token(Some(token));
                        }
                    }
                }
                AuthParams::Scope {
                    namespace,
                    database,
                    scope,
                    params,
                } => {
                    let mut auth_params = if let Value::Object(map) = params {
                        map.clone()
                    } else {
                        serde_json::Map::new()
                    };

                    auth_params.insert("NS".to_string(), Value::String(namespace.clone()));
                    auth_params.insert("DB".to_string(), Value::String(database.clone()));
                    auth_params.insert("SC".to_string(), Value::String(scope.clone()));

                    let message = crate::surreal_client::RpcMessage::new("signin")
                        .with_id(2)
                        .with_params(vec![Value::Object(auth_params)]);

                    let response = engine.rpc(message).await?;

                    if let Value::String(token) = response {
                        if let Some(http_engine) = engine.as_any_mut().downcast_mut::<HttpEngine>()
                        {
                            http_engine.set_token(Some(token));
                        }
                    }
                }
                AuthParams::Token(token) => {
                    let message = crate::surreal_client::RpcMessage::new("authenticate")
                        .with_id(2)
                        .with_params(vec![Value::String(token.clone())]);

                    engine.rpc(message).await?;

                    if let Some(http_engine) = engine.as_any_mut().downcast_mut::<HttpEngine>() {
                        http_engine.set_token(Some(token.clone()));
                    }
                }
            }
        }

        // Check version if enabled
        if self.version_check {
            let message = crate::surreal_client::RpcMessage::new("version").with_id(3);
            let _version = engine.rpc(message).await?;
            // TODO: Add actual version compatibility check
        }

        // Create the immutable client
        Ok(SurrealClient::new(engine, self.namespace, self.database))
    }
    */
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_builder() {
        let conn = SurrealConnection::new()
            .url("ws://localhost:8000")
            .namespace("test_ns")
            .database("test_db")
            .auth_root("root", "root")
            .version_check(false);

        assert_eq!(conn.url, Some("ws://localhost:8000".to_string()));
        assert_eq!(conn.namespace, Some("test_ns".to_string()));
        assert_eq!(conn.database, Some("test_db".to_string()));
        assert!(!conn.version_check);
        assert!(matches!(conn.auth, Some(AuthParams::Root { .. })));
    }

    #[test]
    fn test_dsn_parsing() {
        let conn = SurrealConnection::dsn(
            "ws://root:root@localhost:8000/test_ns/test_db?version_check=false",
        )
        .unwrap();

        assert_eq!(conn.url, Some("ws://localhost:8000".to_string()));
        assert_eq!(conn.namespace, Some("test_ns".to_string()));
        assert_eq!(conn.database, Some("test_db".to_string()));
        assert!(!conn.version_check);
        assert!(matches!(conn.auth, Some(AuthParams::Root { .. })));
    }

    #[test]
    fn test_dsn_with_query_params() {
        let conn =
            SurrealConnection::dsn("http://localhost:8000?namespace=ns&database=db").unwrap();

        assert_eq!(conn.url, Some("http://localhost:8000".to_string()));
        assert_eq!(conn.namespace, Some("ns".to_string()));
        assert_eq!(conn.database, Some("db".to_string()));
    }

    #[test]
    fn test_auth_methods() {
        let conn1 = SurrealConnection::new().auth_root("admin", "pass");
        assert!(matches!(conn1.auth, Some(AuthParams::Root { .. })));

        let conn2 = SurrealConnection::new().auth_namespace("ns_user", "ns_pass");
        assert!(matches!(conn2.auth, Some(AuthParams::Namespace { .. })));

        let conn3 = SurrealConnection::new().auth_database("db_user", "db_pass");
        assert!(matches!(conn3.auth, Some(AuthParams::Database { .. })));

        let conn4 = SurrealConnection::new().auth_token("jwt_token");
        assert!(matches!(conn4.auth, Some(AuthParams::Token(_))));
    }

    #[tokio::test]
    async fn test_connection_to_client_flow() {
        // Example of the new flow: Connection -> authenticate -> creates engine -> returns immutable client

        // This would be the typical usage:
        // let client = Connection::new()
        //     .url("ws://localhost:8000")
        //     .namespace("bakery")
        //     .database("inventory")
        //     .auth_root("root", "root")
        //     .connect()
        //     .await
        //     .unwrap();

        // For testing, we just verify the builder pattern works
        let connection = SurrealConnection::new()
            .url("ws://localhost:8000")
            .namespace("test_namespace")
            .database("test_database")
            .auth_root("admin", "password")
            .version_check(false);

        assert_eq!(connection.url, Some("ws://localhost:8000".to_string()));
        assert_eq!(connection.namespace, Some("test_namespace".to_string()));
        assert_eq!(connection.database, Some("test_database".to_string()));
        assert!(!connection.version_check);
        assert!(matches!(connection.auth, Some(AuthParams::Root { .. })));

        // The client would be immutable once created:
        // - client.query() - no mut needed
        // - client.select() - no mut needed
        // - client.let_var() - changes session but client stays immutable
        // - Multiple clients can be cloned, each with unique session
    }
}
