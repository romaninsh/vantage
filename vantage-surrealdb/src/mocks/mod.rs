//! Mock implementations for SurrealDB testing
//!
//! This module provides simplified mock implementations for SurrealDB
//! that require exact matching of method calls and parameters, making
//! tests predictable and easy to debug.
//!
//! ## Features
//!
//! - **Exact Matching**: All method calls and parameters must match exactly
//! - **Fail-Fast**: Panics with descriptive error messages for unmatched requests
//! - **Builder Pattern**: User-friendly API for setting up test scenarios
//! - **Debug Support**: Optional logging to see what queries are being sent
//!
//! ## Quick Start
//!
//! ```rust
//! use vantage_surrealdb::mocks::SurrealMockBuilder;
//! use serde_json::json;
//!
//! let db = SurrealMockBuilder::new()
//!     .with_query_response("SELECT * FROM users", json!([{"name": "Alice"}]))
//!     .with_method_response("create", json!({"id": "new_record"}))
//!     .build();
//! ```
//!
//! ## Advanced Usage
//!
//! ### Exact Response Matching
//!
//! ```rust
//! use vantage_surrealdb::mocks::SurrealMockBuilder;
//! use serde_json::json;
//!
//! // Exact query matching - query string must match exactly
//! let db = SurrealMockBuilder::new()
//!     .with_query_response("SELECT name, email FROM users", json!([{"name": "John", "email": "john@example.com"}]))
//!     .with_query_response("SELECT name, email FROM ONLY users", json!({"name": "Active User", "email": "active@example.com"}))
//!     .with_query_response("SELECT VALUE email FROM users", json!(["john@example.com", "jane@example.com"]))
//!     .build();
//!
//! // Method-specific responses with exact parameter matching
//! let advanced_db = SurrealMockBuilder::new()
//!     .with_method_response("create", json!({"id": "new_record"}))
//!     .with_exact_response("count", json!({"table": "users"}), json!(42))
//!     .with_debug(true) // Enable debug logging
//!     .build();
//! ```
//!
//! ## Error Handling
//!
//! When a request doesn't match any configured pattern, the mock will panic with a descriptive error:
//!
//! ```text
//! MockSurrealEngine: executed method query(["SELECT * FROM posts",{}]),
//! but allowed patterns are: query(["SELECT * FROM users",{}])
//! ```
//!
//! This makes it easy to identify exactly what query was sent and what patterns are available.

pub mod engine;

pub use engine::{MockSurrealEngine, SurrealMockBuilder};
