//! SurrealDB `UPDATE` statement builder.
//!
//! Builds parameterized `UPDATE` expressions in three modes:
//!
//! - **SET** (default) — `UPDATE target SET key = val, ...`
//! - **CONTENT** — `UPDATE target CONTENT {...}` (replaces all fields)
//! - **MERGE** — `UPDATE target MERGE {...}` (partial update, keeps unmentioned fields)
//!
//! Supports optional `WHERE` conditions for bulk updates.
//!
//! # Examples
//!
//! ```rust,ignore
//! use vantage_surrealdb::{SurrealUpdate, thing::Thing};
//!
//! // SET mode (default) — update specific fields
//! let upd = SurrealUpdate::new(Thing::new("users", "alice"))
//!     .with_field("score", 99i64);
//!
//! // CONTENT mode — replace all fields
//! let upd = SurrealUpdate::new(Thing::new("users", "alice"))
//!     .content()
//!     .with_field("name", "Alice".to_string());
//!
//! // MERGE mode — partial update
//! let upd = SurrealUpdate::new(Thing::new("users", "alice"))
//!     .merge()
//!     .with_field("verified", true);
//!
//! // Bulk update with WHERE
//! let upd = SurrealUpdate::table("users")
//!     .with_field("active", false)
//!     .with_condition(surreal_expr!("last_login < {}", "2020-01-01"));
//!
//! // Execute
//! db.execute(&upd.expr()).await?;
//! ```

pub mod builder;
pub mod render;

#[cfg(test)]
mod tests;

use indexmap::IndexMap;

use crate::Expr;
use crate::types::AnySurrealType;

/// Update mode determines the SurrealDB update strategy.
#[derive(Debug, Clone)]
pub enum UpdateMode {
    /// `UPDATE target SET key = val, ...` — set specific fields
    Set,
    /// `UPDATE target CONTENT {...}` — replace all fields
    Content,
    /// `UPDATE target MERGE {...}` — partial update, keeps unmentioned fields
    Merge,
}

/// Builder for SurrealDB `UPDATE` statements.
///
/// Produces `UPDATE target SET/CONTENT/MERGE ... [WHERE ...]`.
/// All field values are passed as parameterized CBOR values, not inlined strings.
pub struct SurrealUpdate {
    /// Target expression (table name, `Thing`, or arbitrary expression).
    pub target: Expr,
    /// Update strategy: SET, CONTENT, or MERGE.
    pub mode: UpdateMode,
    /// Field key-value pairs in insertion order.
    pub fields: IndexMap<String, AnySurrealType>,
    /// Optional WHERE conditions (combined with AND).
    pub conditions: Vec<Expr>,
}
