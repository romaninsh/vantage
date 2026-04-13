//! Common imports for working with vantage-sql.
//!
//! ```
//! use vantage_sql::prelude::*;
//! ```

pub use std::process::Termination;
pub use vantage_core::{Context, Result as VantageResult, VantageError};
pub use vantage_expressions::{ExprDataSource, Expression, Expressive, Order, Selectable};
pub use vantage_table::column::core::Column;
pub use vantage_table::operation::Operation;

pub use crate::primitives::identifier::{Identifier, ident};

#[cfg(feature = "sqlite")]
pub use crate::sqlite::statements::SqliteSelect;
#[cfg(feature = "sqlite")]
pub use crate::sqlite::{AnySqliteType, SqliteDB};
#[cfg(feature = "sqlite")]
pub use crate::sqlite_expr;
