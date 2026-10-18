// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! # Vantage Table
//!
//! A `Table` is a dataset with columns. This crate defines `Table`, `Column` and
//! the `TableSource` trait that database drivers implement.
//!
//! Type erasure for cross-driver work lives in
//! [`vantage-vista`](https://docs.rs/vantage-vista): wrap a typed table with
//! `db.vista_factory().from_table(table)?` to get a `Vista` that talks
//! `Record<ciborium::Value>` whatever the driver.
//!
//! ```rust,ignore
//! use vantage_table::prelude::Table;
//! use vantage_expressions::expr;
//!
//! let table = Table::new("users", my_datasource)
//!     .with_id_column("id")
//!     .with_column_of::<String>("name")
//!     .with_condition(expr!("age > {}", 18));
//! ```

pub mod traits;

#[cfg(any(test, feature = "mocks"))]
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
pub mod write_context;
