//! # SurrealDB Query Targets
//!
//! doc wip

use vantage_expressions::OwnedExpression;

/// Represents a target in a FROM clause
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_expressions::expr;
/// use vantage_surrealdb::select::target::Target;
///
/// // doc wip
/// let target = Target::new(expr!("users"));
/// ```

#[derive(Debug, Clone)]
pub struct Target {
    target: OwnedExpression,
}

impl Target {
    /// Creates a new query target
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `target` - doc wip
    pub fn new(target: impl Into<OwnedExpression>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl Into<OwnedExpression> for Target {
    fn into(self) -> OwnedExpression {
        self.target
    }
}
