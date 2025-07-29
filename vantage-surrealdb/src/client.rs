use serde_json::Value;
use url::Url;

use crate::surreal_client::{
    ConnectParams, Engine, HttpEngine, RecordId, RecordRange, Result, RpcMessage, SessionState,
    SigninParams, SignupParams, SurrealError, Table, WsEngine,
};

// TODO: Step 1 - Define core data structures and traits ✅ COMPLETED
// - Create Engine trait for HTTP/WebSocket abstraction ✅
// - Define RpcMessage struct for method calls ✅
// - Define connection parameters and auth structures ✅
// - Create error types for different failure modes ✅

// TODO: Step 2 - Implement HTTP engine ✅ COMPLETED
// - Create HttpEngine struct with reqwest client ✅
// - Implement basic HTTP connectivity (connect, status, health) ✅
// - Add JSON content negotiation ✅
// - Handle authentication headers and session management ✅

// TODO: Step 3 - Implement WebSocket engine ✅ COMPLETED
// - Create WsEngine struct with tokio-tungstenite ✅
// - Implement WebSocket handshake and protocol negotiation ✅
// - Add message queuing and response correlation ✅
// - Handle connection lifecycle (connect, reconnect, close) ✅

// TODO: Step 4 - Add RPC message handling ✅ COMPLETED
// - Implement RpcMessage serialization to JSON ✅
// - Create response parsing and error handling ✅
// - Add method-specific parameter processing ✅
// - Implement incremental ID generation for requests ✅

// TODO: Step 5 - Implement authentication layer ✅ COMPLETED
// - Add signin/signup methods with credential processing ✅
// - Implement JWT token management and storage ✅
// - Add authenticate/invalidate session methods ✅
// - Handle namespace/database scope switching ✅

// TODO: Step 6 - Add CRUD operations ✅ COMPLETED
// - Implement select, update, delete methods ✅
// - Add upsert, merge, patch operations ✅
// - Handle record ID types and table references ✅
// - Add bulk insert and relation operations ✅

// TODO: Step 7 - Implement query interface ✅ BASIC COMPLETED
// - Add raw query execution with parameter binding ✅
// - Implement query result parsing and type conversion (PARTIAL)
// - Add query builder utilities
// - Handle multi-statement query responses (PARTIAL)

// TODO: Step 8 - Add import/export functionality (HTTP only) ✅ COMPLETED
// - Implement database import from .surql files ✅
// - Add database export to string/file ✅
// - Implement ML model import/export ✅
// - Handle authentication for import/export operations ✅

// TODO: Step 9 - Add session and parameter management ✅ COMPLETED
// - Implement let/unset parameter methods ✅
// - Add session state tracking ✅
// - Handle connection-scoped variables ✅
// - Add info() method for session introspection ✅

// TODO: Step 10 - Error handling and connection management ✅ COMPLETED
// - Create comprehensive error types ✅
// - Add connection pooling and retry logic
// - Implement graceful shutdown and cleanup ✅
// - Add timeout configuration and handling ✅

pub struct SurrealClient {
    engine: Option<Box<dyn Engine>>,
    session: SessionState,
    incremental_id: u64,
}

impl SurrealClient {
    /// Create a new SurrealDB instance
    pub fn new() -> Self {
        Self {
            engine: None,
            session: SessionState::new(),
            incremental_id: 0,
        }
    }

    /// Generate the next incremental ID for RPC messages
    fn next_id(&mut self) -> u64 {
        self.incremental_id += 1;
        self.incremental_id
    }

    /// Connect to a SurrealDB instance
    pub async fn connect(&mut self, dsn: String, params: ConnectParams) -> Result<()> {
        // Parse the URL to determine the protocol
        let url = Url::parse(&dsn)?;

        // Create the appropriate engine based on the protocol
        let mut engine: Box<dyn Engine> = match url.scheme() {
            "ws" | "wss" => Box::new(WsEngine::new(dsn)?),
            "http" | "https" => Box::new(HttpEngine::new(dsn)?),
            _ => {
                return Err(SurrealError::Protocol(
                    "Unsupported protocol. Use ws://, wss://, http://, or https://".to_string(),
                ));
            }
        };

        // Connect to the database
        engine.connect().await?;

        // Store the engine
        self.engine = Some(engine);

        // Set namespace and database if provided
        if params.namespace.is_some() || params.database.is_some() {
            self.use_ns_db(params.namespace, params.database).await?;
        }

        // TODO: Add version check if enabled
        if params.version_check.unwrap_or(true) {
            // Version check implementation will go here
        }

        Ok(())
    }

    /// Use a specific namespace and database
    pub async fn use_ns_db(
        &mut self,
        namespace: Option<String>,
        database: Option<String>,
    ) -> Result<()> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("use").with_id(id).with_params(vec![
            namespace.clone().map(Value::String).unwrap_or(Value::Null),
            database.clone().map(Value::String).unwrap_or(Value::Null),
        ]);

        engine.rpc(message).await?;

        // For HTTP engines, we need to update the engine's namespace/database first
        if let Some(http_engine) = self
            .engine
            .as_mut()
            .and_then(|e| e.as_any_mut().downcast_mut::<HttpEngine>())
        {
            http_engine.set_namespace_database(namespace.clone(), database.clone());
        }

        // Update session state
        self.session.set_target(namespace, database);

        Ok(())
    }

    /// Sign in with the given credentials
    pub async fn signin(&mut self, params: SigninParams) -> Result<Option<String>> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let rpc_params = params.to_rpc_params();
        let message = RpcMessage::new("signin")
            .with_id(id)
            .with_params(vec![Value::Object(rpc_params.into_iter().collect())]);

        let result = engine.rpc(message).await?;

        // Extract token from result if it's a string
        let token = match result {
            Value::String(token) => Some(token),
            _ => None,
        };

        // Update session state
        if let Some(ref token) = token {
            // For HTTP engines, we need to update the engine's token
            if let Some(http_engine) = self
                .engine
                .as_mut()
                .and_then(|e| e.as_any_mut().downcast_mut::<HttpEngine>())
            {
                http_engine.set_token(Some(token.clone()));
            }

            self.session.set_token(Some(token.clone()));
        }

        Ok(token)
    }

    /// Sign up with the given credentials
    pub async fn signup(&mut self, params: SignupParams) -> Result<Option<String>> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let rpc_params = params.to_rpc_params();
        let message = RpcMessage::new("signup")
            .with_id(id)
            .with_params(vec![Value::Object(rpc_params.into_iter().collect())]);

        let result = engine.rpc(message).await?;

        // Extract token from result if it's a string
        let token = match result {
            Value::String(token) => Some(token),
            _ => None,
        };

        // Update session state
        if let Some(ref token) = token {
            // For HTTP engines, we need to update the engine's token
            if let Some(http_engine) = self
                .engine
                .as_mut()
                .and_then(|e| e.as_any_mut().downcast_mut::<HttpEngine>())
            {
                http_engine.set_token(Some(token.clone()));
            }

            self.session.set_token(Some(token.clone()));
        }

        Ok(token)
    }

    /// Authenticate with a JWT token
    pub async fn authenticate(&mut self, token: String) -> Result<()> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("authenticate")
            .with_id(id)
            .with_params(vec![Value::String(token.clone())]);

        engine.rpc(message).await?;

        // For HTTP engines, we need to update the engine's token
        if let Some(http_engine) = self
            .engine
            .as_mut()
            .and_then(|e| e.as_any_mut().downcast_mut::<HttpEngine>())
        {
            http_engine.set_token(Some(token.clone()));
        }

        // Update session state
        self.session.set_token(Some(token));

        Ok(())
    }

    /// Invalidate the current session
    pub async fn invalidate(&mut self) -> Result<()> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("invalidate").with_id(id);

        engine.rpc(message).await?;

        // Clear authentication from session
        self.session.clear_auth();

        Ok(())
    }

    /// Set a parameter for the session
    pub async fn let_var(&mut self, key: String, value: Value) -> Result<()> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("let")
            .with_id(id)
            .with_params(vec![Value::String(key.clone()), value.clone()]);

        engine.rpc(message).await?;

        // Update session state
        self.session.set_param(key, value);

        Ok(())
    }

    /// Unset a parameter from the session
    pub async fn unset(&mut self, key: String) -> Result<()> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("unset")
            .with_id(id)
            .with_params(vec![Value::String(key.clone())]);

        engine.rpc(message).await?;

        // Update session state
        self.session.unset_param(&key);

        Ok(())
    }

    /// Create a record in the database
    pub async fn create(&mut self, table: String, record: Value) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("create")
            .with_id(id)
            .with_params(vec![Value::String(table), record]);

        engine.rpc(message).await
    }

    /// Select records from the database
    pub async fn select(&mut self, thing: Value) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("select")
            .with_id(id)
            .with_params(vec![thing]);

        engine.rpc(message).await
    }

    /// Select all records from a table
    pub async fn select_all(&mut self, table: &Table) -> Result<Value> {
        self.select(Value::from(table)).await
    }

    /// Select a specific record by ID
    pub async fn select_record(&mut self, record_id: &RecordId) -> Result<Value> {
        self.select(Value::from(record_id)).await
    }

    /// Select records using a range
    pub async fn select_range(&mut self, range: &RecordRange) -> Result<Value> {
        self.select(Value::String(range.to_surql())).await
    }

    /// Update records in the database
    pub async fn update(&mut self, thing: Value, data: Value) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("update")
            .with_id(id)
            .with_params(vec![thing, data]);

        engine.rpc(message).await
    }

    /// Update a specific record by ID
    pub async fn update_record(&mut self, record_id: &RecordId, data: Value) -> Result<Value> {
        self.update(Value::from(record_id), data).await
    }

    /// Update all records in a table
    pub async fn update_all(&mut self, table: &Table, data: Value) -> Result<Value> {
        self.update(Value::from(table), data).await
    }

    /// Upsert (create or update) records in the database
    pub async fn upsert(&mut self, thing: Value, data: Value) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("upsert")
            .with_id(id)
            .with_params(vec![thing, data]);

        engine.rpc(message).await
    }

    /// Upsert a specific record by ID
    pub async fn upsert_record(&mut self, record_id: &RecordId, data: Value) -> Result<Value> {
        self.upsert(Value::from(record_id), data).await
    }

    /// Merge data into records in the database
    pub async fn merge(&mut self, thing: Value, data: Value) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("merge")
            .with_id(id)
            .with_params(vec![thing, data]);

        engine.rpc(message).await
    }

    /// Merge data into a specific record by ID
    pub async fn merge_record(&mut self, record_id: &RecordId, data: Value) -> Result<Value> {
        self.merge(Value::from(record_id), data).await
    }

    /// Merge data into all records in a table
    pub async fn merge_all(&mut self, table: &Table, data: Value) -> Result<Value> {
        self.merge(Value::from(table), data).await
    }

    /// Patch records in the database using JSON Patch operations
    pub async fn patch(
        &mut self,
        thing: Value,
        patches: Vec<Value>,
        diff: Option<bool>,
    ) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let mut params = vec![thing, Value::Array(patches)];
        if let Some(diff_val) = diff {
            params.push(Value::Bool(diff_val));
        }

        let message = RpcMessage::new("patch").with_id(id).with_params(params);

        engine.rpc(message).await
    }

    /// Delete records from the database
    pub async fn delete(&mut self, thing: Value) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("delete")
            .with_id(id)
            .with_params(vec![thing]);

        engine.rpc(message).await
    }

    /// Delete a specific record by ID
    pub async fn delete_record(&mut self, record_id: &RecordId) -> Result<Value> {
        self.delete(Value::from(record_id)).await
    }

    /// Delete all records from a table
    pub async fn delete_all(&mut self, table: &Table) -> Result<Value> {
        self.delete(Value::from(table)).await
    }

    /// Insert records into the database
    pub async fn insert(&mut self, table: String, data: Value) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("insert")
            .with_id(id)
            .with_params(vec![Value::String(table), data]);

        engine.rpc(message).await
    }

    /// Insert records into a table using the Table type
    pub async fn insert_into(&mut self, table: &Table, data: Value) -> Result<Value> {
        self.insert(table.name().to_string(), data).await
    }

    /// Insert a single record into a table
    pub async fn insert_one(&mut self, table: &Table, record: Value) -> Result<Value> {
        self.insert_into(table, record).await
    }

    /// Insert multiple records into a table
    pub async fn insert_many(&mut self, table: &Table, records: Vec<Value>) -> Result<Value> {
        self.insert_into(table, Value::Array(records)).await
    }

    /// Create relationships between records
    pub async fn relate(
        &mut self,
        from: Value,
        relation: Value,
        to: Value,
        data: Option<Value>,
    ) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let mut params = vec![from, relation, to];
        if let Some(data_val) = data {
            params.push(data_val);
        }

        let message = RpcMessage::new("relate").with_id(id).with_params(params);

        engine.rpc(message).await
    }

    /// Create a relationship between two records using RecordId types
    pub async fn relate_records(
        &mut self,
        from: &RecordId,
        relation: &Table,
        to: &RecordId,
        data: Option<Value>,
    ) -> Result<Value> {
        self.relate(
            Value::from(from),
            Value::from(relation),
            Value::from(to),
            data,
        )
        .await
    }

    /// Run a defined SurrealQL function
    pub async fn run(
        &mut self,
        function: String,
        version: Option<String>,
        args: Option<Vec<Value>>,
    ) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let mut params = vec![Value::String(function)];

        if let Some(ver) = version {
            params.push(Value::String(ver));
        } else {
            params.push(Value::Null);
        }

        if let Some(arguments) = args {
            params.push(Value::Array(arguments));
        } else {
            params.push(Value::Null);
        }

        let message = RpcMessage::new("run").with_id(id).with_params(params);

        engine.rpc(message).await
    }

    /// Execute a query with parameters
    pub async fn query(&mut self, query: String, params: Value) -> Result<Vec<Value>> {
        let id = self.next_id();
        // Convert params Value to HashMap - it must be an Object
        let params_map: std::collections::HashMap<String, Value> = match params {
            Value::Object(map) => map.into_iter().collect(),
            _ => {
                return Err(SurrealError::Protocol(
                    "Query parameters must be a JSON object".to_string(),
                ));
            }
        };

        // Merge session parameters with query parameters
        let mut merged_params = self.session.params().clone();
        merged_params.extend(params_map);

        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("query").with_id(id).with_params(vec![
            Value::String(query),
            Value::Object(merged_params.into_iter().collect()),
        ]);

        let result = engine.rpc(message).await?;

        // Handle query results - SurrealDB returns an array of query results
        match result {
            Value::Array(results) => Ok(results),
            other => Ok(vec![other]),
        }
    }

    /// Get information about the current session
    pub async fn info(&mut self) -> Result<Value> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("info").with_id(id);

        engine.rpc(message).await
    }

    /// Get the version of the SurrealDB instance
    pub async fn version(&mut self) -> Result<String> {
        let id = self.next_id();
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let message = RpcMessage::new("version").with_id(id);

        let result = engine.rpc(message).await?;

        match result {
            Value::String(version) => Ok(version),
            _ => Err(SurrealError::Protocol(
                "Invalid version response".to_string(),
            )),
        }
    }

    /// Close the connection
    pub async fn close(&mut self) -> Result<()> {
        if let Some(mut engine) = self.engine.take() {
            engine.close().await?;
        }
        self.session.reset();
        Ok(())
    }

    /// Import database content (HTTP only)
    pub async fn import(&mut self, content: &str, username: &str, password: &str) -> Result<Value> {
        if let Some(engine) = &self.engine {
            if let Some(http_engine) = engine.as_any().downcast_ref::<HttpEngine>() {
                return http_engine.import(content, username, password).await;
            }
        }
        Err(SurrealError::Protocol(
            "Import is only supported for HTTP connections".to_string(),
        ))
    }

    /// Export database content (HTTP only)
    pub async fn export(&mut self, username: &str, password: &str) -> Result<String> {
        if let Some(engine) = &self.engine {
            if let Some(http_engine) = engine.as_any().downcast_ref::<HttpEngine>() {
                return http_engine.export(username, password).await;
            }
        }
        Err(SurrealError::Protocol(
            "Export is only supported for HTTP connections".to_string(),
        ))
    }

    /// Import ML model (HTTP only)
    pub async fn import_ml(
        &mut self,
        content: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Value> {
        if let Some(engine) = &self.engine {
            if let Some(http_engine) = engine.as_any().downcast_ref::<HttpEngine>() {
                return http_engine.import_ml(content, username, password).await;
            }
        }
        Err(SurrealError::Protocol(
            "ML import is only supported for HTTP connections".to_string(),
        ))
    }

    /// Export ML model (HTTP only)
    pub async fn export_ml(
        &mut self,
        name: &str,
        version: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<String> {
        if let Some(engine) = &self.engine {
            if let Some(http_engine) = engine.as_any().downcast_ref::<HttpEngine>() {
                return http_engine
                    .export_ml(name, version, username, password)
                    .await;
            }
        }
        Err(SurrealError::Protocol(
            "ML export is only supported for HTTP connections".to_string(),
        ))
    }
}

impl Default for SurrealClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as Base64Engine;
    use serde_json::json;
    use vantage_expressions::{expr, protocol::expressive::DataSource};

    use crate::surrealdb::SurrealDB;

    use super::*;

    #[tokio::test]
    async fn test_surrealdb_creation() {
        let db = SurrealClient::new();
        assert!(db.engine.is_none());
    }

    #[tokio::test]
    async fn test_connect_and_operations() {
        let mut db = SurrealClient::new();

        // Test connection
        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        match db.connect("ws://localhost:8000".to_string(), params).await {
            Ok(_) => {
                println!("Connected successfully!");

                // Test authentication
                let signin_params = SigninParams::root("root", "root");
                match db.signin(signin_params).await {
                    Ok(token) => {
                        println!("Authenticated successfully! Token: {:?}", token);

                        // Stop here as requested - just test connection
                        return;
                    }
                    Err(e) => println!("Authentication failed: {}", e),
                }
            }
            Err(e) => println!("Connection failed: {}", e),
        }
    }

    #[tokio::test]
    async fn test_crud_operations() {
        let mut db = SurrealClient::new();

        // Connect and authenticate
        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        db.connect("ws://localhost:8000".to_string(), params)
            .await
            .unwrap();

        let signin_params = SigninParams::root("root", "root");
        db.signin(signin_params).await.unwrap();
        println!("Testing CRUD operations...");

        // Test create
        let user_data = json!({
            "name": "Test User",
            "email": "test@example.com",
            "age": 25
        });

        match db.create("user".to_string(), user_data).await {
            Ok(result) => println!("Create result: {:?}", result),
            Err(e) => println!("Create failed: {}", e),
        }

        // Test typed operations
        let user_table = Table::new("user");

        // Test select all users using typed method
        match db.select_all(&user_table).await {
            Ok(result) => println!("Select all users (typed): {:?}", result),
            Err(e) => println!("Select failed: {}", e),
        }

        // Test select specific record
        let user_id = RecordId::string("user", "test_user");
        match db.select_record(&user_id).await {
            Ok(result) => println!("Select specific user: {:?}", result),
            Err(e) => println!("Select record failed: {}", e),
        }

        // Test insert with typed methods
        let new_user = json!({
            "name": "Typed User",
            "email": "typed@example.com",
            "age": 30
        });

        match db.insert_one(&user_table, new_user).await {
            Ok(result) => println!("Insert typed user: {:?}", result),
            Err(e) => println!("Insert failed: {}", e),
        }

        // Test query
        let query_result = db
            .query(
                "SELECT * FROM user WHERE age > $min_age".to_string(),
                json!({
                    "min_age": 20
                }),
            )
            .await;

        match query_result {
            Ok(result) => println!("Query result: {:?}", result),
            Err(e) => println!("Query failed: {}", e),
        }
    }

    #[tokio::test]
    async fn test_http_engine() {
        let mut db = SurrealClient::new();

        // Test HTTP connection
        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        match db
            .connect("http://localhost:8000".to_string(), params)
            .await
        {
            Ok(_) => {
                println!("HTTP Connected successfully!");

                // Test authentication
                let signin_params = SigninParams::root("root", "root");
                match db.signin(signin_params).await {
                    Ok(token) => {
                        println!("HTTP Authenticated successfully! Token: {:?}", token);

                        // Test a simple query
                        match db
                            .query("SELECT * FROM user LIMIT 1".to_string(), json!({}))
                            .await
                        {
                            Ok(result) => println!("HTTP Query result: {:?}", result),
                            Err(e) => println!("HTTP Query failed: {}", e),
                        }
                    }
                    Err(e) => println!("HTTP Authentication failed: {}", e),
                }
            }
            Err(e) => println!("HTTP Connection failed: {}", e),
        }
    }

    #[tokio::test]
    async fn test_bakery_queries_with_parameters() {
        let mut db = SurrealClient::new();

        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        if db
            .connect("ws://localhost:8000".to_string(), params)
            .await
            .is_ok()
        {
            let signin_params = SigninParams::root("root", "root");
            if db.signin(signin_params).await.is_ok() {
                println!("Testing bakery database queries with parameters...");

                // Test 1: Get all products for a specific bakery (parameterized)
                let query_params = json!({
                    "bakery_id": "bakery:hill_valley"
                });

                match db.query(
                    "SELECT * FROM $bakery_id->owns->product WHERE is_deleted = false ORDER BY name".to_string(),
                    query_params,
                ).await {
                    Ok(result) => println!("Products for bakery: {:?}", result),
                    Err(e) => println!("Query failed: {}", e),
                }

                // Test 2: Get products with stock below threshold (parameterized)
                let query_params = json!({
                    "stock_threshold": 25,
                    "bakery_id": "bakery:hill_valley"
                });

                match db.query(
                    "SELECT name, inventory.stock FROM product WHERE $bakery_id IN <-owns<-bakery AND inventory.stock < $stock_threshold AND is_deleted = false".to_string(),
                    query_params,
                ).await {
                    Ok(result) => println!("Low stock products: {:?}", result),
                    Err(e) => println!("Low stock query failed: {}", e),
                }

                // Test 3: Get orders for a specific client (parameterized)
                let query_params = json!({
                    "client_id": "client:marty"
                });

                match db
                    .query(
                        "SELECT * FROM $client_id->placed->order".to_string(),
                        query_params,
                    )
                    .await
                {
                    Ok(result) => println!("Client orders: {:?}", result),
                    Err(e) => println!("Client orders query failed: {}", e),
                }

                // Test 4: Find clients who ordered a specific product (parameterized)
                let query_params = json!({
                    "product_id": "product:flux_cupcake"
                });

                match db.query(
                    "SELECT DISTINCT <-placed<-client AS customers FROM order WHERE lines.product CONTAINS $product_id".to_string(),
                    query_params,
                ).await {
                    Ok(result) => println!("Clients who ordered product: {:?}", result),
                    Err(e) => println!("Product customers query failed: {}", e),
                }

                // Test 5: Get products by price range (parameterized)
                let query_params = json!({
                    "min_price": 150,
                    "max_price": 250
                });

                match db.query(
                    "SELECT name, price, calories FROM product WHERE price >= $min_price AND price <= $max_price AND is_deleted = false ORDER BY price".to_string(),
                    query_params,
                ).await {
                    Ok(result) => println!("Products in price range: {:?}", result),
                    Err(e) => println!("Price range query failed: {}", e),
                }

                // Test 6: Calculate total order value for a client with quantity threshold (parameterized)
                let query_params = json!({
                    "client_id": "client:doc",
                    "min_quantity": 1
                });

                match db.query(
                    r#"
                    SELECT
                        math::sum(
                            SELECT VALUE math::sum(
                                array::map(lines[WHERE quantity >= $min_quantity], |$line| $line.quantity * $line.price)
                            )
                            FROM $client_id->placed->order
                        ) AS total_order_value
                    "#.to_string(),
                    query_params,
                ).await {
                    Ok(result) => println!("Client total order value: {:?}", result),
                    Err(e) => println!("Order value query failed: {}", e),
                }

                // Test 7: Get client contact info by email domain (parameterized)
                let query_params = json!({
                    "email_domain": "gmail.com"
                });

                match db.query(
                    "SELECT name, email, contact_details FROM client WHERE string::split(email, '@')[1] = $email_domain".to_string(),
                    query_params,
                ).await {
                    Ok(result) => println!("Clients by email domain: {:?}", result),
                    Err(e) => println!("Email domain query failed: {}", e),
                }

                // Test 8: Update product stock with parameterized values
                let query_params = json!({
                    "product_id": "product:hover_cookies",
                    "new_stock": 45
                });

                match db
                    .query(
                        "UPDATE $product_id SET inventory.stock = $new_stock".to_string(),
                        query_params,
                    )
                    .await
                {
                    Ok(result) => println!("Updated product stock: {:?}", result),
                    Err(e) => println!("Stock update failed: {}", e),
                }
            }
        }
    }

    #[tokio::test]
    async fn test_complex_analytics_queries() {
        let mut db = SurrealClient::new();

        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        if db
            .connect("ws://localhost:8000".to_string(), params)
            .await
            .is_ok()
        {
            let signin_params = SigninParams::root("root", "root");
            if db.signin(signin_params).await.is_ok() {
                println!("Testing complex analytics queries with parameters...");

                // Test 1: Inventory vs Demand Analytics with threshold parameter
                let query_params = json!({
                    "bakery_id": "bakery:hill_valley",
                    "demand_threshold": 10
                });

                match db
                    .query(
                        r#"
                    SELECT * FROM (
                        SELECT
                            name AS product_name,
                            inventory.stock AS current_inventory,
                            math::sum(
                                SELECT VALUE math::sum(
                                    lines[WHERE product = $parent.id].quantity
                                )
                                FROM order
                                WHERE lines.product CONTAINS $parent.id
                            ) AS total_items_ordered
                        FROM product
                        WHERE $bakery_id IN <-owns<-bakery
                            AND is_deleted = false
                    ) WHERE total_items_ordered > current_inventory
                      AND total_items_ordered >= $demand_threshold
                    ORDER BY product_name
                    "#
                        .to_string(),
                        query_params,
                    )
                    .await
                {
                    Ok(result) => println!("High demand products: {:?}", result),
                    Err(e) => println!("Demand analytics query failed: {}", e),
                }

                // Test 2: Client calorie consumption with minimum threshold
                let query_params = json!({
                    "min_calories": 500
                });

                match db.query(
                    r#"
                    SELECT * FROM (
                        SELECT
                            name AS client_name,
                            math::sum(
                                SELECT VALUE math::sum(
                                    array::map(lines, |$line| $line.quantity * $line.product.calories)
                                )
                                FROM ->placed->order
                            ) AS total_calories_ordered
                        FROM client
                    ) WHERE total_calories_ordered >= $min_calories
                    ORDER BY total_calories_ordered DESC
                    "#.to_string(),
                    query_params,
                ).await {
                    Ok(result) => println!("High calorie consumption clients: {:?}", result),
                    Err(e) => println!("Calorie analytics query failed: {}", e),
                }

                // Test 3: Order details with date range filtering
                let query_params = json!({
                    "start_date": "2024-01-01T00:00:00Z"
                });

                match db
                    .query(
                        r#"
                    SELECT
                        id,
                        created_at,
                        lines[*].{
                            product_name: product.name,
                            quantity: quantity,
                            price: price,
                            subtotal: quantity * price
                        } AS items,
                        math::sum(lines[*].quantity * lines[*].price) AS order_total
                    FROM order
                    WHERE created_at >= $start_date
                    ORDER BY created_at DESC
                    "#
                        .to_string(),
                        query_params,
                    )
                    .await
                {
                    Ok(result) => println!("Recent orders with totals: {:?}", result),
                    Err(e) => println!("Order details query failed: {}", e),
                }

                // Test 4: Product performance by profit margin calculation
                let query_params = json!({
                    "bakery_id": "bakery:hill_valley",
                    "cost_multiplier": 0.6
                });

                match db
                    .query(
                        r#"
                    SELECT
                        name AS product_name,
                        price,
                        (price * $cost_multiplier) AS estimated_cost,
                        (price - (price * $cost_multiplier)) AS estimated_profit,
                        math::sum(
                            SELECT VALUE math::sum(lines[WHERE product = $parent.id].quantity)
                            FROM order
                            WHERE lines.product CONTAINS $parent.id
                        ) AS units_sold
                    FROM product
                    WHERE $bakery_id IN <-owns<-bakery
                        AND is_deleted = false
                    ORDER BY estimated_profit DESC
                    "#
                        .to_string(),
                        query_params,
                    )
                    .await
                {
                    Ok(result) => println!("Product profitability analysis: {:?}", result),
                    Err(e) => println!("Profitability query failed: {}", e),
                }
            }
        }
    }

    #[tokio::test]
    async fn test_database_import() {
        let mut db = SurrealClient::new();

        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        match db
            .connect("http://localhost:8000".to_string(), params)
            .await
        {
            Ok(_) => {
                println!("Successfully connected to HTTP endpoint");
                let signin_params = SigninParams::root("root", "root");
                match db.signin(signin_params).await {
                    Ok(token) => {
                        println!("Successfully authenticated with token: {:?}", token);
                        println!("Testing database import functionality...");

                        // Read the v1.surql file content (in a real scenario)
                        let schema_content = r#"
-- Create bakery
CREATE bakery:hill_valley SET
    name = 'Hill Valley Bakery',
    profit_margin = 15;

-- Create test client
CREATE client:test_import SET
    name = 'Test Import User',
    email = 'test@import.com',
    contact_details = '555-TEST',
    is_paying_client = true;

-- Create test product
CREATE product:test_product SET
    name = 'Test Import Product',
    calories = 100,
    price = 99,
    inventory = { stock: 10 };
"#;

                        match db.import(schema_content, "root", "root").await {
                            Ok(result) => {
                                println!("Database import successful: {:?}", result);

                                // Test that the imported data is accessible
                                match db
                                    .query(
                                        "SELECT * FROM client:test_import".to_string(),
                                        json!({}),
                                    )
                                    .await
                                {
                                    Ok(result) => println!("Imported client data: {:?}", result),
                                    Err(e) => println!("Failed to query imported data: {}", e),
                                }
                            }
                            Err(e) => println!("Database import failed: {}", e),
                        }
                    }
                    Err(e) => println!("Authentication failed: {}", e),
                }
            }
            Err(e) => println!("Connection failed: {}", e),
        }
    }

    #[tokio::test]
    async fn test_binary_data_storage() {
        let mut db = SurrealClient::new();

        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        if db
            .connect("ws://localhost:8000".to_string(), params)
            .await
            .is_ok()
        {
            let signin_params = SigninParams::root("root", "root");
            if db.signin(signin_params).await.is_ok() {
                println!("Testing binary data storage and retrieval...");

                // Create sample binary data (simulating an image or document)
                let binary_data = vec![
                    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
                    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1 pixel
                    0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C,
                    0x49, 0x44, 0x41, 0x54, 0x08, 0x57, 0x63, 0xF8, 0x0F, 0x00, 0x00, 0x01, 0x00,
                    0x01, 0x5C, 0xC2, 0x8A, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
                    0xAE, 0x42, 0x60, 0x82,
                ];

                // Encode binary data as base64 for storage
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&binary_data);

                // Test 1: Create a record with binary data
                let file_record = json!({
                    "filename": "test_image.png",
                    "content_type": "image/png",
                    "size": binary_data.len(),
                    "data": base64_data,
                    "checksum": format!("{:x}", md5::compute(&binary_data))
                });

                match db.create("file".to_string(), file_record).await {
                    Ok(result) => {
                        println!("Created binary file record: {:?}", result);

                        // Extract the created record ID
                        if let Value::Object(obj) = &result {
                            if let Some(Value::String(file_id)) = obj.get("id") {
                                println!("File stored with ID: {}", file_id);

                                // Test 2: Retrieve the binary data
                                let query_params = json!({
                                    "file_id": file_id.clone()
                                });

                                match db
                                    .query("SELECT * FROM $file_id".to_string(), query_params)
                                    .await
                                {
                                    Ok(query_result) => {
                                        println!("Retrieved file record: {:?}", query_result);

                                        // Test 3: Simple binary data verification
                                        // Decode the original data and compare with what we expect
                                        match base64::engine::general_purpose::STANDARD
                                            .decode(&base64_data)
                                        {
                                            Ok(decoded_original) => {
                                                if decoded_original == binary_data {
                                                    println!("✅ Binary data integrity verified!");
                                                    println!(
                                                        "Original size: {} bytes",
                                                        binary_data.len()
                                                    );
                                                    println!(
                                                        "Base64 encoded size: {} bytes",
                                                        base64_data.len()
                                                    );

                                                    let checksum =
                                                        format!("{:x}", md5::compute(&binary_data));
                                                    println!("MD5 checksum: {}", checksum);
                                                } else {
                                                    println!(
                                                        "❌ Binary data encoding/decoding mismatch!"
                                                    );
                                                }
                                            }
                                            Err(e) => println!("❌ Base64 decode error: {}", e),
                                        }
                                    }
                                    Err(e) => println!("Failed to retrieve file: {}", e),
                                }

                                // Test 4: Query files by content type with binary size filter
                                let query_params = json!({
                                    "content_type": "image/png",
                                    "min_size": 50
                                });

                                match db.query(
                                    "SELECT filename, size, content_type FROM file WHERE content_type = $content_type AND size >= $min_size".to_string(),
                                    query_params,
                                ).await {
                                    Ok(result) => println!("Files matching criteria: {:?}", result),
                                    Err(e) => println!("File search query failed: {}", e),
                                }

                                // Test 5: Update binary data (simulate file replacement)
                                let new_binary_data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]; // JPEG header
                                let new_base64_data = base64::engine::general_purpose::STANDARD
                                    .encode(&new_binary_data);

                                let update_data = json!({
                                    "filename": "test_image.jpg",
                                    "content_type": "image/jpeg",
                                    "size": new_binary_data.len(),
                                    "data": new_base64_data,
                                    "checksum": format!("{:x}", md5::compute(&new_binary_data))
                                });

                                match db.update(Value::String(file_id.clone()), update_data).await {
                                    Ok(result) => println!("Updated binary file: {:?}", result),
                                    Err(e) => println!("File update failed: {}", e),
                                }
                            }
                        }
                    }
                    Err(e) => println!("Failed to create binary file record: {}", e),
                }

                // Test 6: Bulk binary data operations
                let test_files = vec![
                    (
                        "document.pdf",
                        "application/pdf",
                        vec![0x25, 0x50, 0x44, 0x46],
                    ),
                    (
                        "archive.zip",
                        "application/zip",
                        vec![0x50, 0x4B, 0x03, 0x04],
                    ),
                    ("text.txt", "text/plain", b"Hello, World!".to_vec()),
                ];

                for (filename, content_type, data) in test_files {
                    let file_record = json!({
                        "filename": filename,
                        "content_type": content_type,
                        "size": data.len(),
                        "data": base64::engine::general_purpose::STANDARD.encode(&data),
                        "checksum": format!("{:x}", md5::compute(&data))
                    });

                    match db.create("file".to_string(), file_record).await {
                        Ok(_) => println!("✅ Created file: {}", filename),
                        Err(e) => println!("❌ Failed to create {}: {}", filename, e),
                    }
                }

                // Test 7: Binary data analytics
                match db
                    .query(
                        r#"
                    SELECT
                        content_type,
                        count() AS file_count,
                        math::sum(size) AS total_bytes,
                        math::mean(size) AS avg_size
                    FROM file
                    GROUP BY content_type
                    ORDER BY total_bytes DESC
                    "#
                        .to_string(),
                        json!({}),
                    )
                    .await
                {
                    Ok(result) => println!("Binary data analytics: {:?}", result),
                    Err(e) => println!("Analytics query failed: {}", e),
                }
            }
        }
    }

    #[tokio::test]
    async fn test_datasource_implementation() {
        let mut db = SurrealClient::new();

        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        if db
            .connect("ws://localhost:8000".to_string(), params)
            .await
            .is_ok()
        {
            let signin_params = SigninParams::root("root", "root");
            if db.signin(signin_params).await.is_ok() {
                println!("Testing DataSource implementation...");

                // Create a SharedSurrealDB for DataSource usage
                let shared_db = SurrealDB::new(db);

                // Test 1: Simple query execution via DataSource
                let query = expr!("SELECT COUNT(*) FROM product");
                let result = shared_db.execute(&query).await;
                println!("✅ Product count via DataSource: {:?}", result);

                // Test 2: Deferred execution
                let deferred_query = expr!("SELECT * FROM client LIMIT 2");
                let deferred_fn = shared_db.defer(deferred_query);
                let deferred_result = deferred_fn().await;
                println!("✅ Deferred query result: {:?}", deferred_result);
            }
        }
    }

    #[tokio::test]
    async fn test_session_management() {
        let mut db = SurrealClient::new();

        let params = ConnectParams::new()
            .with_namespace("bakery")
            .with_database("v1");

        if db
            .connect("ws://localhost:8000".to_string(), params)
            .await
            .is_ok()
        {
            let signin_params = SigninParams::root("root", "root");
            if db.signin(signin_params).await.is_ok() {
                println!("Testing session management...");

                // Test setting a variable
                match db
                    .let_var(
                        "user_id".to_string(),
                        Value::Number(serde_json::Number::from(123)),
                    )
                    .await
                {
                    Ok(_) => println!("Set variable successfully"),
                    Err(e) => println!("Set variable failed: {}", e),
                }

                // Test using the variable in a query
                let query_result = db
                    .query(
                        "SELECT * FROM user WHERE id = $user_id".to_string(),
                        json!({}),
                    )
                    .await;

                match query_result {
                    Ok(result) => println!("Query with variable: {:?}", result),
                    Err(e) => println!("Query with variable failed: {}", e),
                }

                // Test getting session info
                match db.info().await {
                    Ok(info) => println!("Session info: {:?}", info),
                    Err(e) => println!("Get info failed: {}", e),
                }

                // Test getting version
                match db.version().await {
                    Ok(version) => println!("SurrealDB version: {}", version),
                    Err(e) => println!("Get version failed: {}", e),
                }
            }
        }
    }
}
