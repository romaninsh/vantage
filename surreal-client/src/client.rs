use std::sync::Arc;

use ciborium::Value as CborValue;
use serde_json::{Value, json};

use crate::live::LiveStream;
use crate::{Engine, RecordId, RecordRange, Result, SessionState, SurrealError, Table};

/// SurrealDB client. JSON methods transcode through CBOR on the wire; use
/// [`query_cbor`](Self::query_cbor) when type fidelity matters.
pub struct SurrealClient {
    engine: Arc<tokio::sync::Mutex<Box<dyn Engine>>>,
    session: SessionState,
    incremental_id: Arc<std::sync::atomic::AtomicU64>,
    debug: bool,
}

impl Clone for SurrealClient {
    /// A new client sharing the same engine and session
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            session: self.session.clone(),
            incremental_id: self.incremental_id.clone(),
            debug: self.debug,
        }
    }
}

impl SurrealClient {
    /// Create a client over `engine`, targeting the optional namespace and database
    pub fn new(
        engine: Box<dyn Engine>,
        namespace: Option<String>,
        database: Option<String>,
    ) -> Self {
        let mut session = SessionState::new();
        session.set_target(namespace, database);

        Self {
            engine: Arc::new(tokio::sync::Mutex::new(engine)),
            session,
            incremental_id: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            debug: false,
        }
    }

    /// Enable debug mode to log queries
    pub fn with_debug(mut self, enabled: bool) -> Self {
        self.debug = enabled;
        self
    }

    /// Check if debug mode is enabled
    pub fn is_debug(&self) -> bool {
        self.debug
    }

    /// Set a parameter for the session
    pub async fn let_var(&mut self, key: &str, value: Value) -> Result<()> {
        let mut engine = self.engine.lock().await;

        let params = json!([key, value]);

        engine.send_message("let", params).await?;

        self.session.set_param(key.to_string(), value);

        Ok(())
    }

    /// Unset a parameter from the session
    pub async fn unset(&mut self, key: &str) -> Result<()> {
        let mut engine = self.engine.lock().await;

        let params = json!([key]);

        engine.send_message("unset", params).await?;

        self.session.unset_param(key);
        Ok(())
    }

    /// Create a record in the database
    pub async fn create(&self, resource: &str, data: Option<Value>) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = if let Some(data) = data {
            json!([resource, data])
        } else {
            json!([resource])
        };

        engine.send_message("create", params).await
    }

    /// Select records from the database
    pub async fn select(&self, resource: &str) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = json!([resource]);

        engine.send_message("select", params).await
    }

    /// Select all records from a table
    pub async fn select_all(&self, table: Table) -> Result<Value> {
        self.select(&table.to_string()).await
    }

    /// Select a specific record by ID
    pub async fn select_record(&self, record: RecordId) -> Result<Value> {
        self.select(&record.to_string()).await
    }

    /// Select a range of records
    pub async fn select_range(&self, range: RecordRange) -> Result<Value> {
        self.select(&range.to_string()).await
    }

    /// Update records in the database
    pub async fn update(&self, resource: &str, data: Option<Value>) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = if let Some(data) = data {
            json!([resource, data])
        } else {
            json!([resource])
        };

        engine.send_message("update", params).await
    }

    /// Update a specific record by ID
    pub async fn update_record(&self, record: RecordId, data: Value) -> Result<Value> {
        self.update(&record.to_string(), Some(data)).await
    }

    /// Update all records in a table
    pub async fn update_all(&self, table: Table, data: Value) -> Result<Value> {
        self.update(&table.to_string(), Some(data)).await
    }

    /// Upsert (insert or update) records in the database
    pub async fn upsert(&self, resource: &str, data: Option<Value>) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = if let Some(data) = data {
            json!([resource, data])
        } else {
            json!([resource])
        };

        engine.send_message("upsert", params).await
    }

    /// Upsert a specific record by ID
    pub async fn upsert_record(&self, record: RecordId, data: Value) -> Result<Value> {
        self.upsert(&record.to_string(), Some(data)).await
    }

    /// Merge data into records in the database
    pub async fn merge(&self, resource: &str, data: Value) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = json!([resource, data]);

        engine.send_message("merge", params).await
    }

    /// Merge data into a specific record by ID
    pub async fn merge_record(&self, record: RecordId, data: Value) -> Result<Value> {
        self.merge(&record.to_string(), data).await
    }

    /// Merge data into all records in a table
    pub async fn merge_all(&self, table: Table, data: Value) -> Result<Value> {
        self.merge(&table.to_string(), data).await
    }

    /// Apply JSON patches to records
    pub async fn patch(&self, resource: &str, patches: Vec<Value>) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = json!([resource, patches]);

        engine.send_message("patch", params).await
    }

    /// Delete records from the database
    pub async fn delete(&self, resource: &str) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = json!([resource]);

        engine.send_message("delete", params).await
    }

    /// Delete a specific record by ID
    pub async fn delete_record(&self, record: RecordId) -> Result<Value> {
        self.delete(&record.to_string()).await
    }

    /// Delete all records from a table
    pub async fn delete_all(&self, table: Table) -> Result<Value> {
        self.delete(&table.to_string()).await
    }

    /// Insert one record, or an array of records, into a table
    pub async fn insert(&self, table: &str, data: Value) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = json!([table, data]);

        engine.send_message("insert", params).await
    }

    /// Insert multiple records
    pub async fn insert_many(&self, table: Table, data: Vec<Value>) -> Result<Value> {
        self.insert(&table.to_string(), Value::Array(data)).await
    }

    /// Create a relation between records
    pub async fn relate(
        &self,
        from: &str,
        relation: &str,
        to: &str,
        data: Option<Value>,
    ) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = if let Some(data) = data {
            json!([from, relation, to, data])
        } else {
            json!([from, relation, to])
        };

        engine.send_message("relate", params).await
    }

    /// Create a relation between specific records
    pub async fn relate_records(
        &self,
        from: RecordId,
        relation: Table,
        to: RecordId,
        data: Option<Value>,
    ) -> Result<Value> {
        self.relate(
            &from.to_string(),
            &relation.to_string(),
            &to.to_string(),
            data,
        )
        .await
    }

    /// Run a stored function
    pub async fn run(&self, func: &str, args: Option<Value>) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = if let Some(args) = args {
            json!([func, args])
        } else {
            json!([func])
        };

        engine.send_message("run", params).await
    }

    /// Execute a custom SurrealQL query
    pub async fn query(&self, sql: &str, variables: Option<Value>) -> Result<Value> {
        if self.debug {
            if let Some(ref vars) = variables {
                println!("🔍 SQL: {}", sql);
                println!(
                    "📊 Params: {}",
                    serde_json::to_string_pretty(vars).unwrap_or_default()
                );
            } else {
                println!("🔍 SQL: {}", sql);
            }
        }

        let mut engine = self.engine.lock().await;

        let params = if let Some(vars) = variables {
            json!([sql, vars])
        } else {
            json!([sql])
        };

        let response = engine.send_message("query", params).await?;

        if self.debug {
            let icon = if let Value::Array(ref results) = response {
                if results
                    .iter()
                    .any(|r| r.get("status").and_then(|s| s.as_str()) == Some("ERR"))
                {
                    "❌"
                } else {
                    "✅"
                }
            } else {
                "✅"
            };

            println!(
                "{} Response: {}",
                icon,
                serde_json::to_string_pretty(&response).unwrap_or_default()
            );
        }

        match response {
            Value::Array(results) => Ok(Value::Array(results)),
            other => Ok(other),
        }
    }

    /// Get information about the current session
    pub async fn info(&self) -> Result<Value> {
        let mut engine = self.engine.lock().await;

        let params = json!([]);

        engine.send_message("info", params).await
    }

    /// Get the version of the SurrealDB instance
    pub async fn version(&self) -> Result<String> {
        let mut engine = self.engine.lock().await;

        let params = json!([]);

        let response = engine.send_message("version", params).await?;

        match response {
            Value::String(version) => Ok(version),
            _ => Err(SurrealError::Protocol(
                "Invalid version response format".to_string(),
            )),
        }
    }

    /// Close the connection
    pub async fn close(self) -> Result<()> {
        Ok(())
    }

    /// Import database content (HTTP only)
    pub async fn import(&self, _content: &str, _username: &str, _password: &str) -> Result<Value> {
        Err(SurrealError::Protocol(
            "Import is not supported in minimal engine implementation".to_string(),
        ))
    }

    /// Export database content (HTTP only)
    pub async fn export(&self, _username: &str, _password: &str) -> Result<String> {
        Err(SurrealError::Protocol(
            "Export is not supported in minimal engine implementation".to_string(),
        ))
    }

    /// Import ML model (HTTP only)
    pub async fn import_ml(
        &self,
        _content: &str,
        _username: Option<&str>,
        _password: Option<&str>,
    ) -> Result<Value> {
        Err(SurrealError::Protocol(
            "ML import is not supported in minimal engine implementation".to_string(),
        ))
    }

    /// Export ML model (HTTP only)
    pub async fn export_ml(
        &self,
        _name: &str,
        _version: Option<&str>,
        _username: Option<&str>,
        _password: Option<&str>,
    ) -> Result<String> {
        Err(SurrealError::Protocol(
            "ML export is not supported in minimal engine implementation".to_string(),
        ))
    }

    /// Start a live query on a table (or record) and stream change
    /// notifications.
    ///
    /// Issues the `live` RPC, then registers the returned live-query id with
    /// the engine so its read loop forwards every matching `CREATE`/`UPDATE`/
    /// `DELETE` frame onto the returned [`LiveStream`]. Requires a WebSocket
    /// connection — request/response-only engines return an error from
    /// [`Engine::register_live`].
    ///
    /// The stream stays open until the connection closes. Dropping it (or
    /// calling [`LiveStream::close`]) kills the server-side query.
    pub async fn live(&self, resource: &str) -> Result<LiveStream> {
        let mut engine = self.engine.lock().await;

        let params = CborValue::Array(vec![CborValue::Text(resource.to_string())]);
        let response = engine.send_message_cbor("live", params).await?;

        let query_id = cbor_uuid_string(&response).ok_or_else(|| {
            SurrealError::Protocol(format!(
                "live query did not return a usable id: {:?}",
                response
            ))
        })?;

        let rx = engine.register_live(&query_id).await?;
        Ok(LiveStream {
            query_id,
            rx,
            client: Some(self.clone()),
        })
    }

    /// Stop a live query by its id (the [`LiveStream::query_id`]).
    ///
    /// Sends the `kill` RPC and drops the local subscriber so no further
    /// notifications are delivered for that id.
    pub async fn kill(&self, query_id: &str) -> Result<()> {
        let mut engine = self.engine.lock().await;
        let params = CborValue::Array(vec![CborValue::Text(query_id.to_string())]);
        engine.send_message_cbor("kill", params).await?;
        engine.unregister_live(query_id).await;
        Ok(())
    }

    /// Execute a custom SurrealQL query with CBOR parameters
    pub async fn query_cbor(&self, sql: &str, variables: CborValue) -> Result<CborValue> {
        let mut engine = self.engine.lock().await;

        if self.debug {
            println!("SQL: {}", sql);
            println!("Params: {:?}", variables);
        }

        let params = CborValue::Array(vec![CborValue::Text(sql.to_string()), variables]);
        let response = engine.send_message_cbor("query", params).await?;

        if self.debug {
            println!("✅ CBOR Response: {:?}", response);
        }

        Ok(response)
    }
}

/// Render the `live` RPC result (a CBOR UUID) into the same string form the
/// engine derives from notification frame ids, so a live subscription matches
/// its notifications. SurrealDB encodes UUIDs as tag 37 over a 16-byte string;
/// a plain text id is accepted too.
fn cbor_uuid_string(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Text(t) => Some(t.clone()),
        CborValue::Tag(_, inner) => cbor_uuid_string(inner),
        CborValue::Bytes(b) if b.len() == 16 => Some(
            uuid::Uuid::from_slice(b)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| hex::encode(b)),
        ),
        CborValue::Bytes(b) => Some(hex::encode(b)),
        _ => None,
    }
}
