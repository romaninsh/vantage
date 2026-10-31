//! Connection builder: URL or DSN, credentials, then `connect()`.

use crate::{DebugEngine, Engine, Result, SurrealClient, SurrealError, WsCborEngine};

use serde_json::Value;
use tracing::warn;
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
    /// Scope authentication; `connect` rejects it as unsupported
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

    /// Parse a DSN: `scheme://[user:pass@]host[:port][/namespace[/database]]`.
    ///
    /// DSN credentials sign in as root unless `?auth=` (or `?auth_level=`) is
    /// `namespace`/`ns` or `database`/`db`. `?namespace=` and `?database=`
    /// override the path. A DSN without a host is an error.
    pub fn dsn(dsn: impl AsRef<str>) -> Result<Self> {
        let mut conn = Self::new();
        let url = Url::parse(dsn.as_ref())?;

        if url.host().is_none() {
            return Err(SurrealError::Connection(
                "URL must have a valid host".to_string(),
            ));
        }

        // The stored URL keeps no credentials, path or query.
        let base_url = format!("{}://{}", url.scheme(), url.host_str().unwrap());
        let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
        let final_url = format!("{}{}", base_url, port);
        conn.url = Some(final_url);

        if !url.username().is_empty() {
            let username = url.username().to_string();
            let password = url.password().unwrap_or("").to_string();
            conn.auth = Some(AuthParams::Root { username, password });
        }

        let path_segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();

        if let Some(namespace) = path_segments.first().filter(|s| !s.is_empty()) {
            conn.namespace = Some(namespace.to_string());
        }
        if let Some(database) = path_segments.get(1).filter(|s| !s.is_empty()) {
            conn.database = Some(database.to_string());
        }

        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "namespace" => conn.namespace = Some(value.into_owned()),
                "database" => conn.database = Some(value.into_owned()),
                "version_check" => {
                    conn.version_check = value.parse().unwrap_or(true);
                }
                // Cloud instances have no root users, so DSN credentials need
                // a way to sign in at namespace or database level.
                "auth" | "auth_level" => {
                    let (username, password) = match conn.auth.take() {
                        Some(AuthParams::Root { username, password }) => (username, password),
                        other => {
                            conn.auth = other;
                            continue;
                        }
                    };
                    conn.auth = Some(match value.as_ref() {
                        "namespace" | "ns" => AuthParams::Namespace { username, password },
                        "database" | "db" => AuthParams::Database { username, password },
                        "root" => AuthParams::Root { username, password },
                        other => {
                            return Err(SurrealError::Connection(format!(
                                "Unknown auth level `{other}` — use root, namespace, or database"
                            )));
                        }
                    });
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

    /// Set scope authentication. `connect` currently rejects it as an
    /// unsupported method.
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

    pub(crate) async fn init_engine(&self, engine: &mut crate::WsCborEngine) -> Result<()> {
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
            AuthParams::Token(token) => {
                // SurrealDB Cloud brokers per-instance access tokens out of band.
                let auth_params = CborValue::Array(vec![CborValue::Text(token.clone())]);
                engine
                    .send_message_cbor("authenticate", auth_params)
                    .await?;
            }
            _ => {
                return Err(SurrealError::Connection(
                    "Unsupported authentication method".to_string(),
                ));
            }
        }

        // A token is already scoped by its claims, and a database-scoped actor
        // may not re-select the namespace: SurrealDB answers `use` with IAM
        // `NotAllowed`. So only credential logins send `use`.
        let is_token_auth = matches!(self.auth, Some(AuthParams::Token(_)));
        if !is_token_auth && let Some(namespace) = &self.namespace {
            let use_params = CborValue::Array(vec![
                CborValue::Text(namespace.clone()),
                CborValue::Text(self.database.as_ref().unwrap_or(&String::new()).clone()),
            ]);
            engine.send_message_cbor("use", use_params).await?;
        }

        Ok(())
    }

    /// Connect over `ws://`, `wss://` or `cbor://` (an alias for `ws://`), sign
    /// in, select the namespace and database, and return the client. Asks
    /// the server's version once, for
    /// [`supports_transactions`](SurrealClient::supports_transactions).
    pub async fn connect(self) -> Result<SurrealClient> {
        let mut client = self.open().await?;
        client.interactive_transactions = match client.version().await {
            Ok(version) => has_interactive_transactions(&version),
            Err(err) => {
                warn!("SurrealDB version probe failed, transactions write directly: {err}");
                false
            }
        };
        Ok(client)
    }

    /// [`connect`](Self::connect) without the version probe.
    pub(crate) async fn open(self) -> Result<SurrealClient> {
        let url_str = self
            .url
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("URL is required".to_string()))?;
        let url = Url::parse(url_str)
            .map_err(|e| SurrealError::Connection(format!("Invalid URL: {}", e)))?;

        let mut engine: Box<dyn Engine> = match url.scheme() {
            "ws" | "wss" | "cbor" => Box::new(WsCborEngine::from_connection(&self).await?),
            _ => {
                return Err(SurrealError::Protocol(
                    "Unsupported protocol. Use ws://, wss://, or cbor://".to_string(),
                ));
            }
        };

        if self.debug {
            engine = DebugEngine::wrap(engine);
        }

        let debug = self.debug;
        let mut client = SurrealClient::new(engine, self.namespace.clone(), self.database.clone());
        client.connection = Some(self);
        Ok(client.with_debug(debug))
    }
}

/// Whether `version` (`surrealdb-3.2.4+…`) names a server whose RPC has
/// `begin`/`commit`/`cancel`, which came with SurrealDB 3.
fn has_interactive_transactions(version: &str) -> bool {
    let number = version.strip_prefix("surrealdb-").unwrap_or(version);
    number
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 3)
}
