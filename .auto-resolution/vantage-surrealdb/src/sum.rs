//! # SurrealDB Identifiers
//!
//! doc wip

use vantage_expressions::{Expression, Expressive};

use crate::{AnySurrealType, Expr, identifier::Identifier, surreal_expr};

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
    expr: Expr,
}

impl Sum {
    /// Calculate sum of expression
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(expr: Expr) -> Self {
        Self {
            expr: surreal_expr!("math::sum({})", (expr)),
        }
    }
}

impl Expressive<AnySurrealType> for Sum {
    fn expr(&self) -> Expr {
        self.expr.clone()
    }
}

impl From<Sum> for Expr {
    fn from(val: Sum) -> Self {
        val.expr()
    }
}

#[derive(Debug, Clone)]
pub struct Fx {
    name: String,
    expr: Vec<Expr>,
}

impl Fx {
    /// Execute any function
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `identifier` - doc wip
    pub fn new(name: impl Into<String>, expr: Vec<Expr>) -> Self {
        Self {
            name: name.into(),
            expr,
        }
    }
}

impl Expressive<AnySurrealType> for Fx {
    fn expr(&self) -> Expr {
        surreal_expr!(
            "{}({})",
            (Identifier::new(self.name.clone())),
            (Expression::from_vec(self.expr.clone(), ", "))
        )
    }
}

impl From<Fx> for Expr {
    fn from(val: Fx) -> Self {
        val.expr()
    }
}
