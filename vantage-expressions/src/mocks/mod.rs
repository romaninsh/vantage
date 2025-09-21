//! Mock implementations for testing
//!
//! This module provides standardized mock implementations that can be used across
//! all Vantage crates for testing purposes. Instead of duplicating mock code in
//! every test file, use these reusable patterns.
//!
//! ## Available Mock Types
//!
//! ### DataSource Mocks
//!
//! #### 1. StaticDataSource - Always returns the same value
//! ```rust
//! use vantage_expressions::mocks::StaticDataSource;
//! use serde_json::json;
//!
//! let mock = StaticDataSource::new(json!({"status": "ok"}));
//! // Any query will return {"status": "ok"}
//! ```
//!
//! #### 2. PatternDataSource - Maps query patterns to responses
//! ```rust
//! use vantage_expressions::mocks::PatternDataSource;
//! use serde_json::json;
//!
//! let mock = PatternDataSource::new()
//!     .with_pattern("SELECT * FROM users", json!([{"name": "Alice"}]))
//!     .with_pattern("SELECT COUNT(*) FROM orders", json!(42));
//! // Matches exact queries and returns mapped responses
//! ```

pub mod column;
pub mod datasource;
pub mod selectable;

pub use column::MockColumn;
pub use datasource::{FlatteningPatternDataSource, PatternDataSource, StaticDataSource};
