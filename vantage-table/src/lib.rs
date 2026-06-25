//! # Vantage Table
//!
//! Table = DataSet with Columns
//!
//! This crate provides definition for Columns, TableSource - necessary trait for Database SDKs to implement.
//!
//! Type erasure for cross-driver work lives one layer up in [`vantage-vista`](https://docs.rs/vantage-vista):
//! wrap any typed `Table<T, E>` with `T::vista_factory().from_table(...)` to get a `Vista`
//! that talks `Record<ciborium::Value>` regardless of the underlying driver.
//!
//! ## Example
//!
//! ```rust,ignore
//! use vantage_table::{Table, Column, EmptyEntity};
//! use vantage_expressions::expr;
//!
//! // Create a new table with a datasource
//! let mut table = Table::new("users", my_datasource);
//!
//! // Add columns
//! table.add_column(Column::new("name"));
//! table.add_column(Column::new("email").with_alias("user_email"));
//!
//! // Add conditions
//! table.add_condition(expr!("age > {}", 18));
//! table.add_condition(expr!("status = {}", "active"));
//!
//! // Or use the builder pattern
//! let table = Table::new("users", my_datasource)
//!     .with(|t| {
//!         t.add_column(Column::new("name"));
//!         t.add_condition(expr!("active = {}", true));
//!     });
//! ```

pub mod traits;

pub mod mocks;

pub mod active_entity_ext;
pub mod cbor_ext;
pub mod conditions;
pub mod pagination;
pub mod prelude;
pub mod references;
pub mod sorting;

pub mod column;
pub mod source;
pub mod table;
