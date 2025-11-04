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

pub mod flattening;
pub mod r#static;

pub use flattening::FlatteningPatternDataSource;
pub use r#static::StaticDataSource;
