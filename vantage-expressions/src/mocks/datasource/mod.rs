//! DataSource mock implementations for testing
//!
//! This module provides two main patterns for mocking DataSource behavior:
//!
//! ## StaticDataSource
//! Always returns the same value regardless of query:
//! ```rust
//! use vantage_expressions::mocks::StaticDataSource;
//! use serde_json::json;
//!
//! let mock = StaticDataSource::new(json!({"result": "success"}));
//! // Any execute() call returns {"result": "success"}
//! ```
//!
//! ## PatternDataSource
//! Maps query patterns to specific responses:
//! ```rust
//! use vantage_expressions::mocks::PatternDataSource;
//! use serde_json::json;
//!
//! let mock = PatternDataSource::new()
//!     .with_pattern("SELECT * FROM users", json!([{"name": "Alice"}]))
//!     .with_pattern("SELECT COUNT(*) FROM orders", json!(42));
//! ```

pub mod flattening;
pub mod pattern;
pub mod r#static;

pub use flattening::FlatteningPatternDataSource;
pub use pattern::PatternDataSource;
pub use r#static::StaticDataSource;
