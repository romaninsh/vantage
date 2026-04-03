//! SurrealDB `CREATE` statement builder.
//!
//! Builds parameterized `CREATE table SET ...` or `CREATE table:id SET ...`
//! expressions for execution via [`ExprDataSource::execute()`].
//!
//! # Examples
//!
//! ```rust,ignore
//! use vantage_surrealdb::{SurrealInsert, thing::Thing};
//!
//! // Auto-generated ID
//! let ins = SurrealInsert::new("users")
//!     .with_field("name", "Alice".to_string())
//!     .with_field("age", 30i64);
//!
//! // Explicit ID
//! let ins = SurrealInsert::new("users")
//!     .with_id("alice")
//!     .with_field("name", "Alice".to_string());
//!
//! // Thing reference field
//! let ins = SurrealInsert::new("order")
//!     .with_id("o1")
//!     .with_field("customer", Thing::new("user", "alice"));
//!
//! // Execute
//! db.execute(&ins.expr()).await?;
//! ```

pub mod builder;
pub mod render;

#[cfg(test)]
mod tests;

use indexmap::IndexMap;

use crate::identifier::Identifier;
use crate::types::AnySurrealType;

/// Builder for SurrealDB `CREATE` statements.
///
/// Produces `CREATE table SET key = val, ...` or `CREATE table:id SET ...`.
/// All field values are passed as parameterized CBOR values, not inlined strings.
pub struct SurrealInsert {
    /// Target table (auto-escaped if reserved keyword).
    pub table: Identifier,
    /// Optional record ID. When set, produces `CREATE table:id ...`.
    pub id: Option<Identifier>,
    /// Field key-value pairs in insertion order.
    pub fields: IndexMap<String, AnySurrealType>,
}
