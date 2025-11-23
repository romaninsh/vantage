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

pub mod conditions;
pub mod pagination;
pub mod prelude;
// pub mod references;
pub mod sorting;

// pub mod any;

pub mod column;
pub mod source;
pub mod table;

// use async_trait::async_trait;
// use indexmap::IndexMap;
// use std::marker::PhantomData;
// use std::sync::Arc;
// use vantage_expressions::SelectSource;

// use vantage_core::{
//     Result, error,
//     util::error::{Context, vantage_error},
// };
// use vantage_dataset::dataset::{ReadableValueSet, WritableValueSet};
// use vantage_expressions::{AnyExpression, Expression, protocol::selectable::Selectable};

// pub mod any;
// pub mod column_collection;
// pub mod insertable;
// pub mod mocks;
// pub mod models_macro;
// pub mod pagination;
// pub mod prelude;
// pub mod readable;
// pub mod record;
// pub mod references;
// pub mod tablesource;
// pub mod with_columns;
// pub mod with_conditions;
// pub mod with_ordering;
// pub mod with_refs;
// pub mod writable;

// /// Re-export ColumnLike from vantage-expressions for convenience
// pub use crate::tablesource::ColumnLike;
// /// Re-export DataSource from vantage-expressions for convenience
// pub use vantage_expressions::QuerySource;

// pub use crate::column_collection::ColumnCollectionExt;
// pub use crate::pagination::Pagination;
// pub use crate::tablesource::TableSource;
// pub use crate::with_columns::{Column, ColumnFlag};
// pub use crate::with_conditions::ConditionHandle;
// pub use crate::with_ordering::{OrderBy, OrderByExt, OrderHandle, SortDirection};

// // Re-export Entity trait from vantage-core
// pub use vantage_core::Entity;

// /// Empty entity type for tables without a specific entity
// #[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
// pub struct EmptyEntity;

// /// Entity that contains ID only
// #[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
// pub struct IdEntity {
//     pub id: String,
// }
