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

// pub mod associated_query;
// pub mod conditional;

// pub mod field_projection;
pub mod identifier;
pub mod operation;
// pub mod protocol;
pub mod ext;
pub mod macros;
pub mod statements;
pub mod sum;
pub mod surreal_return;
pub mod surrealdb;
// pub mod prelude;
// pub mod column;
// pub mod table;
pub mod thing;
// pub mod typed_expression;
// pub mod variable;

// Re-export statement builders at crate root for convenience
pub use statements::SurrealDelete;
pub use statements::SurrealInsert;
pub use statements::SurrealSelect;
pub use statements::SurrealUpdate;

// Backwards-compat module aliases
pub use statements::delete;
pub use statements::insert;
pub use statements::select;
pub use statements::update;

// SurrealDB expression support using vantage-expressions with AnySurrealType
pub type Expr = vantage_expressions::Expression<AnySurrealType>;

// Add types module
pub mod types;
pub use types::*;

// Re-export extension trait
pub use ext::SurrealTableExt;

// Re-export main SurrealDB types for convenience
pub use ciborium::Value as CborValue;
pub use types::{AnySurrealType, SurrealType, SurrealTypeVariants};
