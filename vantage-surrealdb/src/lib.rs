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
//! ```rust,ignore
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

// pub mod associated_query;
// pub mod conditional;

// pub mod field_projection;
pub mod identifier;
// pub mod insert;
// pub mod operation;
// pub mod protocol;
// pub mod sum;
// pub mod surreal_return;
pub mod macros;
pub mod surrealdb;
// pub mod prelude;
// pub mod column;
// pub mod select;
// pub mod table;
// pub mod thing;
// pub mod typed_expression;
// pub mod variable;

// pub use associated_query::{SurrealAssociated, SurrealAssociatedQueryable};
// pub use column::SurrealColumn;
// pub use insert::SurrealInsert;
// pub use select::SurrealSelect;
// pub use surrealdb::SurrealDB;
// pub use table::{SurrealTableCore, SurrealTableExt};
// pub use typed_expression::TypedExpression;

// SurrealDB expression support using vantage-expressions with AnySurrealType
pub type Expr = vantage_expressions::Expression<AnySurrealType>;

// /// Macro to create SurrealDB expressions with AnySurrealType
// /// Usage: expr!("template", arg1, arg2)
// #[macro_export]
// macro_rules! expr {
//     ($template:expr) => {
//         vantage_expressions::expr_any!(surreal_client::types::AnySurrealType, $template)
//     };
//     ($template:expr, $($param:tt),*) => {
//         vantage_expressions::expr_any!(surreal_client::types::AnySurrealType, $template, $($param),*)
//     };
// }

// Add types module
pub mod types;
pub use types::*;

// Re-export main SurrealDB types for convenience
pub use types::{AnySurrealType, SurrealType, SurrealTypeVariants};
