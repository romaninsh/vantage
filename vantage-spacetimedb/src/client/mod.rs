//! The connection handle.
//!
//! One [`SpacetimeDb`] represents one database on one host. Clones share the
//! inner state, which matters more here than for most drivers: SpacetimeDB
//! subscriptions are **per connection**, keyed by a query-set id, so every table
//! of a database must reach the same handle or each would open its own
//! WebSocket. A page showing eight tables should cost one socket, not eight.

pub mod http;
pub mod ws;

use std::sync::Arc;

use vantage_core::{Result, error};

use crate::schema::{ModuleSchema, SchemaVersion};

/// A SpacetimeDB database, addressed by host and name.
#[derive(Clone)]
pub struct SpacetimeDb {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    /// Base URL, no trailing slash — e.g. `http://127.0.0.1:3000`.
    pub(crate) base_url: String,
    /// Database name or identity, as it appears in `/v1/database/:name_or_identity/…`.
    pub(crate) database: String,
    /// Bearer token. Without one, requests are anonymous: readable public tables
    /// only, and no writes at all.
    pub(crate) token: Option<String>,
    pub(crate) http: reqwest::Client,
}

impl SpacetimeDb {
    /// Address a database on a host. `base_url` may be given with or without a
    /// trailing slash.
    pub fn new(base_url: impl Into<String>, database: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            inner: Arc::new(Inner {
                base_url,
                database: database.into(),
                token: None,
                http: reqwest::Client::new(),
            }),
        }
    }

    /// Attach a bearer token. Anonymous callers may read public tables and
    /// nothing else, so this is what enables writes and private-table access.
    ///
    /// Takes `self` by value and rebuilds the `Arc`, so it must be called before
    /// the handle is shared — which is also the only point at which changing the
    /// credentials of a live shared connection would make sense.
    pub fn with_token(self, token: impl Into<String>) -> Self {
        let inner = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| Inner {
            base_url: arc.base_url.clone(),
            database: arc.database.clone(),
            token: arc.token.clone(),
            http: arc.http.clone(),
        });
        Self {
            inner: Arc::new(Inner {
                token: Some(token.into()),
                ..inner
            }),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    pub fn database(&self) -> &str {
        &self.inner.database
    }

    /// Whether a token was supplied. Necessary for writes, but **not
    /// sufficient** — see [`can_write`](Self::can_write).
    pub fn has_token(&self) -> bool {
        self.inner.token.is_some()
    }

    /// Whether this connection may run SQL `INSERT`/`UPDATE`/`DELETE`.
    ///
    /// SpacetimeDB restricts DML over the SQL endpoint to the database
    /// **owner**, so an ordinary authenticated caller is refused with
    /// *"not authorized to run SQL DML statements"*. Deciding the write
    /// capabilities from `has_token()` alone would therefore advertise writes
    /// that always fail — precisely the kind of lying flag the subscription
    /// contract warns against.
    ///
    /// Costs one request, so callers that build several tables should ask once
    /// and reuse the answer.
    pub async fn can_write(&self) -> Result<bool> {
        Ok(http::database_info(&self.inner).await?.caller_is_owner())
    }

    /// Fetch and normalise the module schema, preferring the newer ABI.
    ///
    /// v10 is tried first because it is the only version that reports event
    /// tables; a host that refuses it falls back to v9, and the resulting
    /// [`ModuleSchema::event_detection`] reports `false` so the loss is visible
    /// rather than silent.
    pub async fn module_schema(&self) -> Result<ModuleSchema> {
        match self.schema_at(SchemaVersion::V10).await {
            Ok(schema) => Ok(schema),
            Err(v10_error) => {
                let schema = self.schema_at(SchemaVersion::V9).await.map_err(|v9_error| {
                    error!(
                        "could not read the SpacetimeDB module schema at either ABI version",
                        v10_error = v10_error.to_string(),
                        v9_error = v9_error.to_string()
                    )
                })?;
                tracing_warn_v9_fallback(&v10_error);
                Ok(schema)
            }
        }
    }

    /// Fetch and normalise the module schema at one specific ABI version.
    pub async fn schema_at(&self, version: SchemaVersion) -> Result<ModuleSchema> {
        let json = http::schema_json(&self.inner, version).await?;
        match version {
            SchemaVersion::V10 => ModuleSchema::from_v10_json(&json),
            SchemaVersion::V9 => ModuleSchema::from_v9_json(&json),
        }
    }

    /// Run one or more SQL statements and return the host's raw JSON response.
    ///
    /// Each statement answers with its own `schema` (a `ProductType`) and `rows`.
    /// Decoding those into records belongs to the table shell; this stays raw so
    /// the CLI example and diagnostics can show exactly what the host said.
    ///
    /// Remember the dialect is narrow: `SELECT` with projections, `WHERE`,
    /// `LIMIT`, `COUNT(*)` and joins, plus `INSERT`/`UPDATE`/`DELETE` — but no
    /// `ORDER BY`, `OFFSET`, `GROUP BY`, `IN` or `LIKE`.
    pub async fn sql(&self, statement: &str) -> Result<String> {
        http::sql_json(&self.inner, statement).await
    }

    /// Open a subscription for one query and stream its changes.
    ///
    /// `row_type` comes from the module schema: pushed rows are bare BSATN and
    /// carry no schema of their own, unlike a SQL response.
    pub(crate) async fn subscribe(
        &self,
        query: String,
        row_type: spacetimedb_sats::ProductType,
        table: String,
    ) -> Result<ws::Subscription> {
        ws::subscribe(&self.inner, query, row_type, table).await
    }

    /// Invoke a reducer with a positional JSON argument array.
    ///
    /// This is the write path a module actually sanctions — reducers enforce the
    /// module's own rules, which raw DML bypasses.
    pub async fn call_reducer(&self, reducer: &str, args: &serde_json::Value) -> Result<String> {
        http::call_reducer(&self.inner, reducer, args).await
    }
}

/// Record the fallback at warn level: it is not fatal, but it silently costs
/// event-table detection, and a permanently-empty grid is a confusing symptom to
/// debug without this line in the log.
fn tracing_warn_v9_fallback(v10_error: &vantage_core::VantageError) {
    tracing::warn!(
        target: "vantage_spacetimedb",
        error = %v10_error,
        "host did not serve module schema v10; falling back to v9 — event tables \
         cannot be detected at this ABI version",
    );
}
