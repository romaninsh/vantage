//! DataSource mock implementations for testing.
//!
//! Modules:
//! - [`mock_data_source`] - Basic `DataSource` marker trait mock
//! - [`mock_expr_data_source`] - `ExprDataSource` with configurable return values
//! - [`mock_selectable_data_source`] - `SelectableDataSource` with configurable return values
//!
//! ## MockDataSource
//!
//! Minimal DataSource implementation for unit testing.
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::*;
//!
//! let mock = MockDataSource::new();
//! ```
//!
//! ## MockExprDataSource
//!
//! ExprDataSource implementation that returns configurable values.
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
//!
//! ## MockSelectableDataSource
//!
//! SelectableDataSource implementation with configurable return values for select queries.
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::*;
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let mock = MockSelectableDataSource::new(json!([
//!     {"id": "flux_cupcake", "name": "Flux Capacitor Cupcake", "price": 120},
//!     {"id": "delorean_donut", "name": "DeLorean Doughnut", "price": 135}
//! ]));
//! let mut select = mock.select();
//! select.set_source("product".to_string(), None);
//! let products = mock.execute_select(&select).await.unwrap();
//! assert_eq!(products.len(), 2);
//! assert_eq!(products[0]["name"], "Flux Capacitor Cupcake");
//! # });
//! ```
//!

pub mod mock_data_source;
pub mod mock_expr_data_source;
pub mod mock_selectable_data_source;

pub use mock_data_source::MockDataSource;
pub use mock_expr_data_source::MockExprDataSource;
pub use mock_selectable_data_source::MockSelectableDataSource;
