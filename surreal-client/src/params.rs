use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Parameters for connecting to SurrealDB
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ConnectParams {
    /// Namespace to use
    pub namespace: Option<String>,

    /// Database to use
    pub database: Option<String>,

    /// Whether to check SurrealDB version compatibility
    pub version_check: Option<bool>,
}

impl ConnectParams {
    /// Create new connection parameters
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the namespace
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set the database
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Enable or disable version checking
    pub fn with_version_check(mut self, check: bool) -> Self {
        self.version_check = Some(check);
        self
    }
}

/// Parameters for authentication
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SigninParams {
    /// Username for root/namespace/database authentication
    pub user: Option<String>,

    /// Password for root/namespace/database authentication
    pub pass: Option<String>,

    /// Namespace for authentication
    pub namespace: Option<String>,

    /// Database for authentication
    pub database: Option<String>,

    /// Scope for record-level authentication (legacy)
    pub scope: Option<String>,

    /// Access method for record-level authentication (v2.0+)
    pub access: Option<String>,

    /// Additional authentication variables
    #[serde(flatten)]
    pub vars: HashMap<String, serde_json::Value>,
}

impl SigninParams {
    /// Create new signin parameters
    pub fn new() -> Self {
        Self::default()
    }

    /// Create root user authentication
    pub fn root(user: impl Into<String>, pass: impl Into<String>) -> Self {
        Self {
            user: Some(user.into()),
            pass: Some(pass.into()),
            ..Default::default()
        }
    }

    /// Create namespace user authentication
    pub fn namespace(
        user: impl Into<String>,
        pass: impl Into<String>,
        namespace: impl Into<String>,
    ) -> Self {
        Self {
            user: Some(user.into()),
            pass: Some(pass.into()),
            namespace: Some(namespace.into()),
            ..Default::default()
        }
    }

    /// Create database user authentication
    pub fn database(
        user: impl Into<String>,
        pass: impl Into<String>,
        namespace: impl Into<String>,
        database: impl Into<String>,
    ) -> Self {
        Self {
            user: Some(user.into()),
            pass: Some(pass.into()),
            namespace: Some(namespace.into()),
            database: Some(database.into()),
            ..Default::default()
        }
    }

    /// Create scope-based authentication (legacy)
    pub fn scope(
        namespace: impl Into<String>,
        database: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            namespace: Some(namespace.into()),
            database: Some(database.into()),
            scope: Some(scope.into()),
            ..Default::default()
        }
    }

    /// Create access-based authentication (v2.0+)
    pub fn access(
        namespace: impl Into<String>,
        database: impl Into<String>,
        access: impl Into<String>,
    ) -> Self {
        Self {
            namespace: Some(namespace.into()),
            database: Some(database.into()),
            access: Some(access.into()),
            ..Default::default()
        }
    }

    /// Add an authentication variable
    pub fn with_var(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.vars.insert(key.into(), value);
        self
    }

    /// Convert to SurrealDB's signin map: `user`, `pass`, `NS`, `DB`, `SC` and
    /// `AC` for the fields that are set, plus every var
    pub fn to_rpc_params(&self) -> HashMap<String, serde_json::Value> {
        let mut params = HashMap::new();

        if let Some(ref user) = self.user {
            params.insert("user".to_string(), serde_json::Value::String(user.clone()));
        }
        if let Some(ref pass) = self.pass {
            params.insert("pass".to_string(), serde_json::Value::String(pass.clone()));
        }

        if let Some(ref namespace) = self.namespace {
            params.insert(
                "NS".to_string(),
                serde_json::Value::String(namespace.clone()),
            );
        }
        if let Some(ref database) = self.database {
            params.insert(
                "DB".to_string(),
                serde_json::Value::String(database.clone()),
            );
        }
        if let Some(ref scope) = self.scope {
            params.insert("SC".to_string(), serde_json::Value::String(scope.clone()));
        }
        if let Some(ref access) = self.access {
            params.insert("AC".to_string(), serde_json::Value::String(access.clone()));
        }

        for (key, value) in &self.vars {
            params.insert(key.clone(), value.clone());
        }

        params
    }
}

/// Parameters for signup operations
pub type SignupParams = SigninParams;
