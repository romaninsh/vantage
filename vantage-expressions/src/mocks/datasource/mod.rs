//! DataSource mock implementations for testing.
//!
//! Modules:
//! - [`datasource`] - Basic `DataSource` marker trait mock
//! - [`querysource`] - `QuerySource` with configurable return values
//! - [`selectsource`] - `SelectSource` with configurable return values
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
//! ## MockQuerySource
//!
//! QuerySource implementation that returns configurable values.
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::*;
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let mock = MockQuerySource::new(json!({"destination_year": 1885}));
//! let query = expr!("CALL time_travel_destination('doc_brown')");
//! let result = mock.execute(&query).await.unwrap();
//! assert_eq!(result, json!({"destination_year": 1885}));
//! # });
//! ```
//!
//! ## MockSelectSource
//!
//! SelectSource implementation with configurable return values for select queries.
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::*;
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let mock = MockSelectSource::new(json!([
//!     {"id": "flux_cupcake", "name": "Flux Capacitor Cupcake", "price": 120},
//!     {"id": "delorean_donut", "name": "DeLorean Doughnut", "price": 135}
//! ]));
//! let mut select = mock.select();
//! select.set_source("product", None);
//! let products = mock.execute_select(&select).await.unwrap();
//! assert_eq!(products.len(), 2);
//! assert_eq!(products[0]["name"], "Flux Capacitor Cupcake");
//! # });
//! ```
//!

pub mod datasource;
pub mod querysource;
pub mod selectsource;

pub use datasource::MockDataSource;
pub use querysource::MockQuerySource;
pub use selectsource::MockSelectSource;
