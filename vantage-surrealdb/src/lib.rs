//! # Vantage SurrealDB Extension
//!
//! Extends Vantage by adding Query Builders for SurrealDB. Standard
//! functionality is implemented through protocol, advanced SurrealDB-only
//! features are implemented as part of default impl.
//!
//! ## Features
//!
//! Implements the following database interaction protocols:
//!
//! - SurrealSelect implements SelectQuery
//!
//! ## Quick Start
//!
//!
//! ```rust
//! use vantage_expressions::{expr, protocol::selectable::Selectable};
//! use vantage_surrealdb::select::SurrealSelect;
//!
//! // doc wip
//! let mut select = SurrealSelect::new();
//! select.set_source(expr!("users"), None);
//! ```
//!
//! ## Modules
//!
//! - [`select`] - doc wip
//! - [`conditional`] - doc wip
//! - [`identifier`] - doc wip
//! - [`thing`] - doc wip
//! - [`variable`] - doc wip
//! - [`protocol`] - doc wip

// TODO: will implement associated queries later
// pub mod associated_query;
pub mod conditional;
pub mod field_projection;
pub mod identifier;
pub mod operation;
pub mod protocol;
pub mod sum;
pub mod surreal_return;
pub mod surrealdb;
// pub mod query;
pub mod prelude;
pub mod select;
pub mod thing;
pub mod variable;

pub use select::SurrealSelect;
pub use surrealdb::SurrealDB;

// Re-export main SurrealDB types for convenience
