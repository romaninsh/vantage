//! SurrealDB `DELETE` statement builder.
//!
//! Builds parameterized `DELETE target [WHERE ...]` expressions.
//!
//! # Examples
//!
//! ```rust,ignore
//! use vantage_surrealdb::{SurrealDelete, thing::Thing, surreal_expr};
//!
//! // Delete a single record
//! let del = SurrealDelete::new(Thing::new("users", "john"));
//!
//! // Delete all records in a table
//! let del = SurrealDelete::table("sessions");
//!
//! // Conditional delete
//! let del = SurrealDelete::table("logs")
//!     .with_condition(surreal_expr!("level = {}", "debug"))
//!     .with_condition(surreal_expr!("age > {}", 30i64));
//!
//! // Execute
//! db.execute(&del.expr()).await?;
//! ```

pub mod builder;
pub mod render;

#[cfg(test)]
mod tests;

use crate::Expr;

/// Builder for SurrealDB `DELETE` statements.
///
/// Produces `DELETE target` or `DELETE target WHERE ...`.
/// Multiple conditions are combined with AND.
pub struct SurrealDelete {
    /// Target expression (table name, `Thing`, or arbitrary expression).
    pub target: Expr,
    /// Optional WHERE conditions (combined with AND).
    pub conditions: Vec<Expr>,
}
