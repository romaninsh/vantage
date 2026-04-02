//! Prelude module for vantage-table
//!
//! This module re-exports commonly used traits and types for convenient importing.

// Core table types
pub use crate::table::Table;

// Column functionality
pub use crate::column::collection::ColumnCollectionExt;
pub use crate::column::core::Column;
pub use crate::column::flags::ColumnFlag;

// Traits
pub use crate::traits::column_like::ColumnLike;
pub use crate::traits::table_like::TableLike;
pub use crate::traits::table_source::TableSource;

// Ordering functionality
pub use crate::sorting::{OrderBy, SortDirection};
pub use crate::table::sorting::OrderByExt;

// Pagination functionality
pub use crate::pagination::Pagination;

// Conditions
pub use crate::conditions::ConditionHandle;

// Mock functionality for testing
pub use crate::mocks::mock_table_source::MockTableSource;

// TODO: Re-enable these when modules are implemented
// Record functionality
// pub use crate::record::{Record, RecordTable};

// Type aliases
pub use crate::any::AnyRecord;

// Reference functionality
pub use crate::any::AnyTable;
// pub use crate::references::{ReferenceMany, ReferenceOne, RelatedTable};

// Model macros
// pub use crate::models;
