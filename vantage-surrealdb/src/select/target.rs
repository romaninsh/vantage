//! # SurrealDB Query Targets
//!
//! doc wip

use crate::Expr;

/// Represents a target in a FROM clause
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::{select::target::Target, surreal_expr};
///
/// let target = Target::new(surreal_expr!("users"));
/// ```

#[derive(Debug, Clone)]
pub struct Target {
    target: Expr,
}

impl Target {
    /// Creates a new query target
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `target` - doc wip
    pub fn new(target: impl Into<Expr>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl From<Target> for Expr {
    fn from(val: Target) -> Self {
        val.target
    }
}
