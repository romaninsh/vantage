//! # Vantage Table
//!
//! Table = DataSet with Columns
//!
//! This crate provides definition for Columns, TableSource - necessary trait for Database SDKs to implement.
//!
//! Additionally this crate implements generic Table struct and AnyTable type-erasing wrapper.
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

pub mod cbor_ext;
pub mod conditions;
pub mod pagination;
pub mod prelude;
pub mod references;
pub mod sorting;

pub mod any;

pub mod column;
pub mod source;
pub mod table;

// TODO: Re-enable when 0.3 migration is complete
// pub mod models_macro;
// pub mod record;
// pub mod references;
// pub mod with_columns;
