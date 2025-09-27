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

use crate::{SurrealDB, associated_query::SurrealAssociated, surreal_return::SurrealReturn};
use vantage_table::{Entity, Table};

mod core;
mod readable;
// mod queryable;
// mod transform;
// mod writable;

pub use core::*;
// pub use queryable::*;
// pub use transform::*;
// pub use writable::*;

/// Extension trait for Table<SurrealDB, E> providing SurrealDB-specific functionality
///
/// This trait combines all the modular traits into a single interface for convenience.
/// The actual implementations are done through the standard dataset traits:
/// - WritableDataSet<E> and InsertableDataSet<E> in writable.rs
/// - ReadableDataSet<E> is implemented in vantage-table using our SurrealTableSelectable
/// - Transform operations in transform.rs
/// - ID operations in with_id.rs
#[async_trait::async_trait]
pub trait SurrealTableExt<E: Entity> {
    /// Create a count query that returns the number of rows
    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64>;
}

#[async_trait::async_trait]
impl<E: Entity> SurrealTableExt<E> for Table<SurrealDB, E> {
    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64> {
        todo!()
        // let count_return = self.select_surreal().query.as_count();
        // SurrealAssociated::new(count_return, self.data_source().clone())
    }
}
