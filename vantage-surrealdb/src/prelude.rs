//! Prelude module for vantage-surrealdb
//!
//! This module re-exports commonly used types and traits to simplify imports.
//! Import this to get access to the most frequently used items in one go:
//!
//! ```rust
//! use vantage_surrealdb::prelude::*;
//! ```

// Core database types
pub use crate::select::SurrealSelect;
pub use crate::surrealdb::SurrealDB;

// Essential traits
pub use crate::operation::{Expressive, RefOperation};

// SurrealDB-specific types
pub use crate::identifier::Identifier;
pub use crate::thing::Thing;
pub use crate::variable::Variable;

// Protocol traits from vantage-expressions that are commonly used
pub use vantage_expressions::protocol::DataSource;
pub use vantage_expressions::protocol::expressive::IntoExpressive;
pub use vantage_expressions::protocol::selectable::Selectable;

// Expression utilities
pub use vantage_expressions::{Expr, Expression, expr};

// Common surreal-client types
pub use surreal_client::{SurrealClient, SurrealConnection};

pub use crate::SurrealAssociated;
pub use crate::SurrealAssociatedQueryable;
pub use crate::SurrealColumn;
pub use crate::SurrealColumnOperations;
pub use crate::SurrealTableExt;
