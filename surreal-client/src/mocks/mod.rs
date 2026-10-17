//! Mock engine for tests: every request must match a configured method and
//! params exactly, or the engine panics naming the request and every allowed
//! pattern.
//!
//! ```rust
//! use surreal_client::mocks::SurrealMockBuilder;
//! use serde_json::json;
//!
//! let client = SurrealMockBuilder::new()
//!     .with_query_response("SELECT * FROM users", json!([{"name": "Alice"}]))
//!     .with_exact_response("count", json!({"table": "users"}), json!(42))
//!     .build();
//! ```
//!
//! An unmatched request panics with:
//!
//! ```text
//! MockSurrealEngine: executed method query(["SELECT * FROM posts"]),
//! but allowed patterns are: query(["SELECT * FROM users"])
//! ```

pub mod engine;

pub use engine::{CallLog, MockSurrealEngine, SurrealMockBuilder};
