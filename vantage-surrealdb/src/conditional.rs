//! # Conditional Expressions
//!
//! doc wip

use vantage_expressions::{OwnedExpression, expr};

/// SurrealDB conditional expression (IF-THEN-ELSE)
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_expressions::expr;
/// use vantage_surrealdb::conditional::Conditional;
///
/// // doc wip
/// let conditional = Conditional::new(
///     expr!("age >= 18"),
///     expr!("'adult'"),
///     expr!("'minor'")
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Conditional {
    condition: OwnedExpression,
    then_expr: OwnedExpression,
    else_expr: OwnedExpression,
}

impl Conditional {
    /// Creates a new conditional expression
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `condition` - doc wip
    /// * `then_expr` - doc wip
    /// * `else_expr` - doc wip
    pub fn new(
        condition: impl Into<OwnedExpression>,
        then_expr: impl Into<OwnedExpression>,
        else_expr: impl Into<OwnedExpression>,
    ) -> Self {
        Self {
            condition: condition.into(),
            then_expr: then_expr.into(),
            else_expr: else_expr.into(),
        }
    }
}

impl Into<OwnedExpression> for Conditional {
    fn into(self) -> OwnedExpression {
        expr!(
            "IF ({}) THEN ({}) ELSE ({}) END",
            self.condition,
            self.then_expr,
            self.else_expr
        )
    }
}
