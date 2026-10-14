//! Mock datasources for testing query execution without a database:
//! [`datasource`] mocks, the [`select`] builder and the pattern-based
//! [`mockbuilder`].
//!
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::*;
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let mock = MockExprDataSource::new(json!({"destination_year": 1885}));
//! let query = expr!("CALL time_travel_destination('doc_brown')");
//! let result = mock.execute(&query).await.unwrap();
//! assert_eq!(result, json!({"destination_year": 1885}));
//! # });
//! ```

pub mod datasource;
pub mod mock_builder;
pub mod select;

pub use datasource::{MockDataSource, MockExprDataSource, MockSelectableDataSource};
pub use mock_builder::MockBuilder;
pub use select::MockSelect;

// Alias for backward compatibility with documentation examples
pub use mock_builder as mockbuilder;
