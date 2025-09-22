//! # SurrealDB Identifiers
//!
//! doc wip

use vantage_expressions::{Expression, expr};

use crate::{identifier::Identifier, operation::Expressive};

/// SurrealDB identifier with automatic escaping
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::identifier::Identifier;
///
/// // doc wip
/// let id = Identifier::new("user_name");
/// let escaped = Identifier::new("SELECT"); // Reserved keyword
/// ```
#[derive(Debug, Clone)]
pub struct Sum {
    expr: Expression,
}

impl Sum {
    /// Calculate sum of expression
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(expr: Expression) -> Self {
        Self {
            expr: expr!("math::sum({})", expr),
        }
    }
}

impl Expressive for Sum {
    fn expr(&self) -> Expression {
        self.expr.clone()
    }
}

impl From<Sum> for Expression {
    fn from(val: Sum) -> Self {
        val.expr()
    }
}

#[derive(Debug, Clone)]
pub struct Fx {
    name: String,
    expr: Vec<Expression>,
}

impl Fx {
    /// Execute any function
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(name: impl Into<String>, expr: Vec<Expression>) -> Self {
        Self {
            name: name.into(),
            expr,
        }
    }
}

impl Expressive for Fx {
    fn expr(&self) -> Expression {
        expr!(
            "{}({})",
            Identifier::new(self.name.clone()),
            Expression::from_vec(self.expr.clone(), ", ")
        )
    }
}

impl From<Fx> for Expression {
    fn from(val: Fx) -> Self {
        val.expr()
    }
}
