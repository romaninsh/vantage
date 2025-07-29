//! # SurrealDB Identifiers
//!
//! doc wip

use vantage_expressions::{OwnedExpression, expr};

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
    expr: OwnedExpression,
}

impl Sum {
    /// Calculate sum of expression
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(expr: OwnedExpression) -> Self {
        Self {
            expr: expr!("math::sum({})", expr),
        }
    }
}

impl Expressive for Sum {
    fn expr(&self) -> OwnedExpression {
        self.expr.clone()
    }
}

impl Into<OwnedExpression> for Sum {
    fn into(self) -> OwnedExpression {
        self.expr()
    }
}

#[derive(Debug, Clone)]
pub struct Fx {
    name: String,
    expr: Vec<OwnedExpression>,
}

impl Fx {
    /// Execute any function
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(name: impl Into<String>, expr: Vec<OwnedExpression>) -> Self {
        Self {
            name: name.into(),
            expr,
        }
    }
}

impl Expressive for Fx {
    fn expr(&self) -> OwnedExpression {
        expr!(
            "{}({})",
            Identifier::new(self.name.clone()),
            OwnedExpression::from_vec(self.expr.clone(), ", ")
        )
    }
}

impl Into<OwnedExpression> for Fx {
    fn into(self) -> OwnedExpression {
        self.expr()
    }
}
