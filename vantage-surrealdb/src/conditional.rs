//! # Conditional Expressions
//!
//! doc wip

use vantage_expressions::{Expression, expr};

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
    condition: Expression,
    then_expr: Expression,
    else_expr: Expression,
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
        condition: impl Into<Expression>,
        then_expr: impl Into<Expression>,
        else_expr: impl Into<Expression>,
    ) -> Self {
        Self {
            condition: condition.into(),
            then_expr: then_expr.into(),
            else_expr: else_expr.into(),
        }
    }
}

impl From<Conditional> for Expression {
    fn from(val: Conditional) -> Self {
        expr!(
            "IF ({}) THEN ({}) ELSE ({}) END",
            val.condition,
            val.then_expr,
            val.else_expr
        )
    }
}
