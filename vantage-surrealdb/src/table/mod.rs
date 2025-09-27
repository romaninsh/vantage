//! # SurrealDB Table Extensions
//!
//! This module provides SurrealDB-specific extensions for `Table<SurrealDB, E>`.
//! The extensions implement standard Vantage dataset traits for better integration:
//!
//! - **ReadableDataSet** - Implemented in vantage-table (uses our SurrealTableSelectable)
//! - **WritableDataSet** - Implemented here for SurrealDB-specific write operations
//! - **InsertableDataSet** - Implemented here for SurrealDB-specific insert operations
//! - **Transform operations** - Functional-style data transformations
//! - **ID operations** - SurrealDB Thing-based ID handling

mod core;
pub mod ext;
mod readable;
pub use core::*;
pub use ext::*;
